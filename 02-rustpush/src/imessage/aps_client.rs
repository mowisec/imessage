use std::{
    collections::HashSet,
    io::Cursor,
    path::PathBuf,
    pin::Pin,
    process::id,
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use log::{debug, error, info, warn};
use plist::{Data, Dictionary, Value};
use serde::{Deserialize, Serialize};
use serde_json;
use tokio::{
    select,
    sync::{broadcast, Mutex},
    task::JoinHandle,
};
use uuid::Uuid;

use crate::{
    aps::{get_message, APSConnection, APSInterestToken},
    ids::{
        identity_manager::{IDSSendMessage, MessageTarget, SendJob},
        user::{IDSNGMIdentity, IDSService},
        CertifiedContext,
    },
    imessage::messages::{AckMessage, ErrorMessage},
    util::{
        bin_deserialize_opt_vec, duration_since_epoch, encode_hex, plist_to_bin, plist_to_string,
        ungzip, global_message_map, MessageTimings,
    },
    APSMessage, ConversationData, IDSUser, Message, MessageInst, NormalMessage, OSConfig,
    PushError,
};

use crate::ids::IDSRecvMessage;
use crate::ids::{
    identity_manager::{DeliveryHandle, IdentityManager, IdentityResource},
    user::{IDSUserIdentity, QueryOptions},
};
use async_recursion::async_recursion;
use chrono::Utc;
use rand::RngCore;
use std::fs::OpenOptions;
use std::io::Write;
use std::str::FromStr;

fn log_measurement(msg: &str) {
    error!("{msg}");
    if let Err(err) = OpenOptions::new()
        .create(true)
        .append(true)
        .open("measurement.log")
        .and_then(|mut file| writeln!(file, "[{}] {msg}", Utc::now().to_rfc3339()))
    {
        error!("Failed to write to measurement.log: {err}");
    }
}

pub const MADRID_SERVICE: IDSService = IDSService {
    name: "com.apple.madrid",
    sub_services: &[
        "com.apple.private.alloy.sms",
        "com.apple.private.alloy.gelato",
        "com.apple.private.alloy.biz",
        "com.apple.private.alloy.gamecenter.imessage",
    ],
    client_data: &[
        ("is-c2k-equipment", Value::Boolean(true)),
        ("optionally-receive-typing-indicators", Value::Boolean(true)),
        ("show-peer-errors", Value::Boolean(true)),
        ("supports-ack-v1", Value::Boolean(true)),
        ("supports-activity-sharing-v1", Value::Boolean(true)),
        ("supports-audio-messaging-v2", Value::Boolean(true)),
        ("supports-autoloopvideo-v1", Value::Boolean(true)),
        ("supports-be-v1", Value::Boolean(true)),
        ("supports-ca-v1", Value::Boolean(true)),
        ("supports-fsm-v1", Value::Boolean(true)),
        ("supports-fsm-v2", Value::Boolean(true)),
        ("supports-fsm-v3", Value::Boolean(true)),
        ("supports-ii-v1", Value::Boolean(true)),
        ("supports-impact-v1", Value::Boolean(true)),
        ("supports-inline-attachments", Value::Boolean(true)),
        ("supports-keep-receipts", Value::Boolean(true)),
        ("supports-location-sharing", Value::Boolean(true)),
        ("supports-media-v2", Value::Boolean(true)),
        ("supports-photos-extension-v1", Value::Boolean(true)),
        ("supports-st-v1", Value::Boolean(true)),
        ("supports-update-attachments-v1", Value::Boolean(true)),
        ("supports-people-request-messages", Value::Boolean(true)),
        ("supports-people-request-messages-v2", Value::Boolean(true)),
        ("supports-people-request-messages-v3", Value::Boolean(true)),
        ("supports-rem", Value::Boolean(true)),
        ("nicknames-version", Value::Real(1.0)),
        ("ec-version", Value::Real(1.0)),
        ("supports-cross-platform-sharing", Value::Boolean(true)),
        ("supports-original-timestamp-v1", Value::Boolean(true)),
        ("supports-sa-v1", Value::Boolean(true)),
        ("supports-photos-extension-v2", Value::Boolean(true)),
        ("prefers-sdr", Value::Boolean(false)),
        ("supports-shared-exp", Value::Boolean(true)),
        ("supports-protobuf-payload-data-v2", Value::Boolean(true)),
        ("supports-hdr", Value::Boolean(true)),
        ("supports-heif", Value::Boolean(true)),
        ("supports-dq-nr", Value::Boolean(true)),
        (
            "supports-family-invite-message-bubble",
            Value::Boolean(true),
        ),
        ("supports-live-delivery", Value::Boolean(true)),
        ("supports-findmy-plugin-messages", Value::Boolean(true)),
        ("supports-stick-moji-backs", Value::Boolean(true)),
        ("supports-emoji-tapbacks", Value::Boolean(true)),
        ("supports-send-later-messages", Value::Boolean(true)),
        ("supports-certified-delivery-v1", Value::Boolean(true)),
        ("supports-transcript-backgrounds", Value::Boolean(true)),
        ("supports-gti", Value::Boolean(true)),
        ("supports-polls", Value::Boolean(true)),
    ],
    flags: 17,
    capabilities_name: "Messenger",
};

impl IDSRecvMessage {
    pub fn to_message(
        &self,
        conversation: Option<ConversationData>,
        message: Message,
    ) -> Result<MessageInst, PushError> {
        let Self {
            sender,
            uuid: Some(uuid),
            ns_since_epoch: Some(ns_since_epoch),
            token,
            send_delivered,
            ..
        } = self
        else {
            return Err(PushError::BadMsg);
        };
        Ok(MessageInst {
            timing_info: None,
            sender: sender.clone(),
            id: Uuid::from_bytes(uuid.clone().try_into().unwrap())
                .to_string()
                .to_uppercase(),
            sent_timestamp: ns_since_epoch / 1000000,
            conversation,
            message,
            target: token.clone().map(|token| vec![MessageTarget::Token(token)]),
            send_delivered: send_delivered.unwrap_or(false),
            verification_failed: self.verification_failed,
            certified_context: self.certified_context(),
        })
    }

    pub fn certified_context(&self) -> Option<CertifiedContext> {
        let Self {
            certified_delivery_receipt: Some(receipt),
            certified_delivery_version: Some(version),
            sender: Some(sender),
            target: Some(target),
            token: Some(token),
            uuid: Some(uuid),
            ..
        } = self
        else {
            return None;
        };
        Some(CertifiedContext {
            version: *version,
            receipt: receipt.clone(),
            sender: sender.clone(),
            target: target.clone(),
            uuid: uuid.clone(),
            token: token.clone(),
        })
    }
}

pub struct IMClient {
    pub conn: APSConnection,
    pub identity: IdentityManager,
    os_config: Arc<dyn OSConfig>,
    _interest_token: APSInterestToken,
}

impl IMClient {
    pub async fn new(
        conn: APSConnection,
        users: Vec<IDSUser>,
        identity: IDSNGMIdentity,
        services: &'static [&'static IDSService],
        cache_path: PathBuf,
        os_config: Arc<dyn OSConfig>,
        mut keys_updated: Box<dyn FnMut(Vec<IDSUser>) + Send + Sync>,
    ) -> IMClient {
        let interest = conn
            .request_topics(vec!["com.apple.private.alloy.sms", "com.apple.madrid"])
            .await
            .0;
        let _ = Self::setup_conn(&conn).await;

        let mut to_refresh = conn.generated_signal.subscribe();
        let reconn_conn = Arc::downgrade(&conn);
        tokio::spawn(async move {
            loop {
                match to_refresh.recv().await {
                    Ok(()) => {
                        let Some(conn) = reconn_conn.upgrade() else {
                            break;
                        };
                        let _ = Self::setup_conn(&conn).await;
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
        });

        let identity = IdentityResource::new(
            users,
            identity,
            services,
            cache_path,
            conn.clone(),
            os_config.clone(),
        )
        .await;

        let mut to_refresh = identity.generated_signal.subscribe();
        let my_ident_ref = identity.resource.clone();
        tokio::spawn(async move {
            loop {
                match to_refresh.recv().await {
                    Ok(()) => keys_updated(my_ident_ref.users.read().await.clone()),
                    Err(broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
        });

        IMClient {
            _interest_token: interest,
            conn,
            os_config: os_config.clone(),
            identity,
        }
    }

    pub fn os_config(&self) -> Arc<dyn OSConfig> {
        self.os_config.clone()
    }

    async fn setup_conn(conn: &APSConnection) -> Result<(), PushError> {
        if let Err(_) = tokio::time::timeout(
            Duration::from_millis(500),
            conn.wait_for_timeout(conn.subscribe().await, |msg| {
                if let APSMessage::NoStorage = msg {
                    Some(())
                } else {
                    None
                }
            }),
        )
        .await
        {
            debug!("Flushing cache!");

            #[derive(Serialize)]
            struct FlushCacheMsg {
                c: u64,
                e: u64,
            }

            let msg = FlushCacheMsg {
                c: 160,
                e: duration_since_epoch().as_nanos() as u64,
            };

            conn.send_message("com.apple.madrid", plist_to_bin(&msg).unwrap(), None)
                .await?;
        }
        Ok(())
    }

    pub async fn handle(&self, msg: APSMessage) -> Result<Option<MessageInst>, PushError> {
        info!("Received APS message: {:?}", msg);
        let mut aps_epoch_ns: Option<u64> = None;
        if let APSMessage::Notification { payload, .. } = &msg {
            info!("APS payload (hex): {}", encode_hex(payload));
            match plist::Value::from_reader(Cursor::new(payload)) {
                Ok(value) => {
                    if let Some(epoch) = value
                        .as_dictionary()
                        .and_then(|dict| dict.get("e"))
                        .and_then(|val| val.as_unsigned_integer())
                    {
                        aps_epoch_ns = Some(epoch);
                    }
                    info!("incoming log: {:?}", value);
                }
                Err(err) => info!("incoming log: <unparseable> {err}"),
            }
        }
        if self.identity.handle(msg.clone()).await? {
            return Ok(Some(MessageInst {
                id: Uuid::new_v4().to_string(),
                sender: None,
                conversation: None,
                message: Message::PeerCacheInvalidate,
                sent_timestamp: 0,
                target: None,
                send_delivered: false,
                verification_failed: false,
                certified_context: None,

                timing_info: None,
            }));
        }
        if let Some(received) = self
            .identity
            .receive_message(msg, &["com.apple.madrid", "com.apple.private.alloy.sms"])
            .await?
        {
            let push_token = received.token.as_ref().map(|t| encode_hex(t));

            let mut recieved = self.process_msg(received).await;
       
       
            {

                let since_the_epoch = duration_since_epoch();

                let mut map = global_message_map.lock().unwrap();
                if let Ok(Some(msg)) = recieved.as_mut() {

                    if let Some(token_hex) = push_token.as_ref() {
                        let lookup_id = match &msg.message {
                            Message::Error(error_message) => parse_reference_message_id(
                                &error_message.for_uuid,
                            )
                            .unwrap_or_else(|| msg.id.clone()),
                            _ => msg.id.clone(),
                        };
                        if let Some(message_timing) = map
                            .get_mut(&lookup_id)
                            .and_then(|x| x.iter_mut().find(|t| &t.token == token_hex))
                        {
                            message_timing.ts_measured_received = since_the_epoch.as_millis() as u64;
                            if let Some(aps_epoch) = aps_epoch_ns {
                                message_timing.ts_server = aps_epoch;
                            }
                            msg.timing_info = Some(message_timing.clone());
                            let delta_ms = message_timing
                                .ts_measured_received
                                .saturating_sub(message_timing.ts_measured_after_send);
                            let aps_epoch_str = aps_epoch_ns
                                .map(|val| format!("{val}"))
                                .unwrap_or_else(|| "-".to_string());
                            let aps_epoch_delta_str = aps_epoch_ns
                                .map(|val| {
                                    let aps_ms = val / 1_000_000;
                                    aps_ms
                                        .saturating_sub(message_timing.ts_measured_after_send)
                                        .to_string()
                                })
                                .unwrap_or_else(|| "-".to_string());
                            let aps_epoch_delta_ms = aps_epoch_ns.map(|val| {
                                let aps_ms = val / 1_000_000;
                                aps_ms.saturating_sub(message_timing.ts_measured_after_send)
                            });
                            let victim_to_aps_ms = aps_epoch_delta_ms
                                .map(|val| delta_ms.saturating_sub(val))
                                .unwrap_or(0);
                            log_measurement(&format!(
                                "received message response from token {} after {} ms (aps_e={} aps_e_delta_ms={} aps_c={} message_id={}) victim to aps was {} ms",
                                message_timing.token,
                                delta_ms,
                                aps_epoch_str,
                                aps_epoch_delta_str,
                                msg.message.get_c(),
                                msg.id,
                                victim_to_aps_ms
                            ));
                            //info!("received message response from token {}, {}, {}", message_timing.token, message_timing.ts_measured_received, message_timing.ts_measured_after_send);
                        };
                    }
                };
                
            }
            recieved            
        } else {
            Ok(None)
        }
    }

    async fn process_msg(
        &self,
        mut payload: IDSRecvMessage,
    ) -> Result<Option<MessageInst>, PushError> {
        let command = payload.command;
        // delivered/read
        if payload.command == 255 {
            let token_hex = payload.token.as_ref().map(|value| encode_hex(value));
            let uuid_str = payload.uuid.as_ref().map(|value| format_uuid_bytes(value));
            let message = Message::Ack(AckMessage {
                status: payload.status,
                error_status: payload.error_status,
                token: token_hex,
                uuid: uuid_str.clone(),
                internal_id: payload.internal_id,
            });
            let id = uuid_str.unwrap_or_else(|| Uuid::new_v4().to_string().to_uppercase());
            let sent_timestamp = payload
                .ns_since_epoch
                .map(|ns| ns / 1_000_000)
                .unwrap_or(0);
            return Ok(Some(MessageInst {
                id,
                sender: payload.sender.clone(),
                conversation: None,
                message,
                sent_timestamp,
                target: payload
                    .token
                    .clone()
                    .map(|token| vec![MessageTarget::Token(token)]),
                send_delivered: payload.send_delivered.unwrap_or(false),
                verification_failed: payload.verification_failed,
                certified_context: payload.certified_context(),
                timing_info: None,
            }));
        }
        if let IDSRecvMessage {
            command: 101 | 102 | 113,
            sender,
            target,
            ..
        } = &payload
        {
            let conversation = if let (Some(sender), Some(target)) = (sender, target) {
                Some(ConversationData {
                    participants: vec![sender.clone(), target.clone()],
                    cv_name: None,
                    sender_guid: None,
                    after_guid: None,
                })
            } else {
                None
            };
            return Ok(payload
                .to_message(
                    conversation,
                    match command {
                        101 => Message::Delivered,
                        102 => Message::Read,
                        113 => Message::NotifyAnyways,
                        _ => panic!("no"),
                    },
                )
                .ok());
        }

        if let IDSRecvMessage {
            sender: Some(sender),
            target: Some(target),
            is_typing: Some(0),
            message: None,
            ..
        } = &payload
        {
            return Ok(payload
                .to_message(
                    Some(ConversationData {
                        participants: vec![sender.clone(), target.clone()],
                        cv_name: None,
                        sender_guid: None,
                        after_guid: None,
                    }),
                    Message::Typing(true, None),
                )
                .ok());
        }

        // errors
        if let IDSRecvMessage {
            command: 120,
            error_for: Some(_),
            error_status: Some(error_status),
            error_string: Some(error_string),
            error_for_str: Some(for_str),
            sender: Some(sender),
            target: Some(target),
            token,
            ..
        } = &payload
        {
            if error_string == "ec-com.apple.messageprotection-802" {
                // refreshing identity cache can fix this
                let mut cache_lock = self.identity.cache.lock().await;
                cache_lock.invalidate(&target, &sender);
            }
            return Ok(payload
                .to_message(
                    None,
                    Message::Error(ErrorMessage {
                        for_uuid: for_str.clone(),
                        status: *error_status,
                        status_str: error_string.clone(),
                        token: token.as_ref().map(|value| encode_hex(value)),
                    }),
                )
                .ok());
        }

        if let IDSRecvMessage {
            command: 130,
            sender: Some(sender),
            target: Some(target),
            token: Some(sender_token),
            ..
        } = &payload
        {
            let mut cache_lock = self.identity.cache.lock().await;
            cache_lock.invalidate(&target, &sender);
            return Ok(None);
        }

        if let IDSRecvMessage {
            command: 145,
            no_reply: None | Some(false),
            sender: Some(sender),
            ..
        } = &payload
        {
            let _ = self
                .send(&mut MessageInst::new(
                    ConversationData {
                        participants: vec![sender.clone()],
                        cv_name: None,
                        sender_guid: Some(Uuid::new_v4().to_string()),
                        after_guid: None,
                    },
                    &sender,
                    Message::MessageReadOnDevice,
                ))
                .await;
        }

        if payload.message_unenc.is_none() {
            if let Some(context) = payload.certified_context() {
                // we weren't delivered, but we got this
                self.identity
                    .certify_delivery("com.apple.madrid", &context, false)
                    .await?;
            }
            return Ok(None);
        }

        let plist_payload = payload
            .message_unenc
            .as_ref()
            .and_then(|body| body.to_plist().ok());
        let raw_payload_json = plist_payload
            .as_ref()
            .and_then(|plist| serde_json::to_string_pretty(plist).ok());
        let raw_bytes = payload
            .message_unenc
            .as_ref()
            .and_then(|body| body.to_bytes())
            .map(|bytes| encode_hex(bytes));

        match MessageInst::from_raw(
            payload.message_unenc.take().unwrap().plist()?,
            &payload,
            &self.conn,
        )
        .await
        {
            Err(PushError::BadMsg) => {
                if let Some(context) = payload.certified_context() {
                    // we weren't delivered, but we got this
                    self.identity
                        .certify_delivery("com.apple.madrid", &context, false)
                        .await?;
                }
                Ok(None)
            }
            Err(err) => Err(err),
            Ok(msg) => {
                if let Some(raw) = raw_payload_json {
                    info!("decrypted message plist: {raw}");
                }
                if let Some(plist) = plist_payload {
                    match plist_to_string(&plist) {
                        Ok(xml) => info!("decrypted message plist (xml): {xml}"),
                        Err(err) => warn!("Failed to stringify decrypted plist: {err}"),
                    }
                }
                if let Some(bytes) = raw_bytes {
                    info!("decrypted message raw hex: {bytes}");
                }
                Ok(Some(msg))
            }
        }
    }

    pub async fn send(&self, message: &mut MessageInst) -> Result<SendJob, PushError> {
        self.send_with_cache_policy(message, false).await
    }

    pub async fn send_using_cached_targets(
        &self,
        message: &mut MessageInst,
    ) -> Result<SendJob, PushError> {
        self.send_with_cache_policy(message, true).await
    }

    async fn send_with_cache_policy(
        &self,
        message: &mut MessageInst,
        prefer_cache: bool,
    ) -> Result<SendJob, PushError> {
        let handles = self.identity.get_handles().await;

        let topic = if message.message.is_sms() {
            "com.apple.private.alloy.sms"
        } else {
            "com.apple.madrid"
        };

        let targets = message.prepare_send(&handles);
        let handle = message.sender.as_ref().unwrap().to_string();
        let mut message_targets = if prefer_cache {
            let ident_cache = self.identity.cache.lock().await;
            let cached_targets = if let Some(message_targets) = &message.target {
                ident_cache.get_targets(topic, &handle, &targets, message_targets)
            } else {
                Ok(ident_cache.get_participants_targets(topic, &handle, &targets))
            };
            cached_targets.ok().filter(|targets| !targets.is_empty())
        } else {
            None
        };

        if message_targets.is_none() {
            self.identity
                .cache_keys(
                    topic,
                    &targets,
                    message.sender.as_ref().unwrap(),
                    false,
                    &QueryOptions {
                        required_for_message: true,
                        result_expected: true,
                    },
                )
                .await?;

            let ident_cache = self.identity.cache.lock().await;
            message_targets = Some(if let Some(message_targets) = &message.target {
                ident_cache.get_targets(topic, &handle, &targets, message_targets)?
            } else {
                ident_cache.get_participants_targets(topic, &handle, &targets)
            });
        }
        let message_targets = message_targets.unwrap_or_default();
        // if we have multiple people, but not a single target going to not us, we cannot "send" this message.
        if targets.len() > 1
            && !message_targets
                .iter()
                .any(|target| !handles.contains(&target.participant))
        {
            return Err(PushError::NoValidTargets);
        }

        let my_handles = self.identity.get_handles().await;

        if message.is_queued() {
            let mut targets = message_targets.clone();
            targets.retain(|t| t.participant == handle);

            let ids_message = message.get_ids(&my_handles, &self.conn, false).await?;
            let sendjob = self
                .identity
                .send_message(topic, ids_message, targets)
                .await;

            if !message.message.should_schedule() {
                // we aren't actually sending this. It is just a draft
                return sendjob;
            }
        }

        let ids_message = message.get_ids(&my_handles, &self.conn, true).await?;

        self.identity
            .send_message(topic, ids_message, message_targets)
            .await
    }
}

fn parse_reference_message_id(for_uuid: &str) -> Option<String> {
    let trimmed = for_uuid.trim();
    if trimmed.is_empty() {
        return None;
    }

    let candidate = if let Some(stripped) = trimmed.strip_prefix("p:") {
        if let Some((_, uuid)) = stripped.split_once('/') {
            if uuid.is_empty() {
                stripped
            } else {
                uuid
            }
        } else {
            stripped
        }
    } else {
        trimmed
    };

    Some(candidate.to_uppercase())
}

fn format_uuid_bytes(bytes: &[u8]) -> String {
    if bytes.len() == 16 {
        let mut buffer = [0u8; 16];
        buffer.copy_from_slice(bytes);
        Uuid::from_bytes(buffer).to_string().to_uppercase()
    } else {
        encode_hex(bytes)
    }
}
