use chrono::Utc;
use log::info;
use plist::Value;
use rusqlite::Connection;
use rustpush::findmy::MULTIPLEX_SERVICE;
use rustpush::macos::MacOSConfig;
use rustpush::util::{decode_hex, duration_since_epoch, global_message_map};
use rustpush::{
    util::{encode_hex, plist_to_string, ungzip},
    APSConnectionResource, APSState, Attachment, AttachmentType, ConversationData, DeleteTarget,
    EditMessage, ErrorMessage, IDSNGMIdentity, IDSUser, IMClient, IconChangeMessage,
    IndexedMessagePart, Message, MessageInst, MessagePart, MessageParts, MessageTarget, MessageType,
    MMCSFile, MoveToRecycleBinMessage, NormalMessage, OperatedChat, PartExtension,
    PermanentDeleteMessage, ReactMessage, ReactMessageType, Reaction, RenameMessage,
    SetTranscriptBackgroundMessage, ShareProfileMessage, TextFormat, UnsendMessage,
    UpdateExtensionMessage, UpdateProfileMessage, UpdateProfileSharingMessage, MADRID_SERVICE,
};
use serde::{Deserialize, Serialize};
use serde_json;
use std::fs;
use std::fs::OpenOptions;
use std::io;
use std::io::Write;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tokio::fs as tokio_fs;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::broadcast::error::RecvError;
use tokio::task::JoinHandle;
use tokio::time::Instant;

fn log_measurement(msg: &str) {
    info!("{msg}");
    if let Err(err) = OpenOptions::new()
        .create(true)
        .append(true)
        .open("measurement.log")
        .and_then(|mut file| writeln!(file, "[{}] {msg}", Utc::now().to_rfc3339()))
    {
        info!("Failed to write to measurement.log: {err}");
    }
}

#[derive(Deserialize)]
struct HwInfo {
    #[serde(rename = "os_config")]
    os_config: MacOSConfig,
    push: APSState,
    identity: IDSNGMIdentity,
}

pub async fn login(bluebubble_dir: &str) -> IMClient {
    let hw_info_data = tokio_fs::read(format!("{}/hw_info.plist", bluebubble_dir))
        .await
        .expect("Failed to read hw_info.plist");
    let hw_info: HwInfo =
        plist::from_bytes(&hw_info_data).expect("Failed to deserialize hw_info.plist");

    let config = Arc::new(hw_info.os_config);
    let push_state = hw_info.push;
    let identity = hw_info.identity;

    let users_data = tokio_fs::read(format!("{}/id.plist", bluebubble_dir))
        .await
        .expect("Failed to read id.plist");
    let users: Vec<IDSUser> =
        plist::from_bytes(&users_data).expect("Failed to deserialize id.plist");

    let (connection, error) = APSConnectionResource::new(config.clone(), Some(push_state)).await;
    if let Some(error) = error {
        panic!("Failed to create APS connection: {}", error);
    }

    let services = &[&MADRID_SERVICE, &MULTIPLEX_SERVICE];

    IMClient::new(
        connection.clone(),
        users,
        identity,
        services,
        std::path::PathBuf::from(format!("{}/id_cache.plist", bluebubble_dir)),
        config.clone(),
        Box::new(|_updated_keys| {}),
    )
    .await
}

pub fn parse_participants(base: &str, extras: Option<&str>) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    let mut participants = Vec::new();

    let push_unique =
        |p: String, seen: &mut std::collections::HashSet<String>, out: &mut Vec<String>| {
            if seen.insert(p.clone()) {
                out.push(p);
            }
        };

    push_unique(base.to_string(), &mut seen, &mut participants);

    if let Some(extra_list) = extras {
        for handle in extra_list
            .split(',')
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
        {
            push_unique(handle.to_string(), &mut seen, &mut participants);
        }
    }

    participants
}

pub fn parse_reaction(kind: &str, emoji_value: Option<&str>) -> Result<Reaction, String> {
    match kind.to_lowercase().as_str() {
        "love" | "heart" => Ok(Reaction::Heart),
        "like" => Ok(Reaction::Like),
        "dislike" => Ok(Reaction::Dislike),
        "laugh" => Ok(Reaction::Laugh),
        "emphasize" | "emphasise" | "emphasis" | "emphasized" => Ok(Reaction::Emphasize),
        "question" | "questioned" => Ok(Reaction::Question),
        "emoji" => emoji_value
            .map(|v| Reaction::Emoji(v.to_string()))
            .ok_or_else(|| "Emoji reaction requires --emoji <value>".to_string()),
        other => Err(format!("Unknown reaction: {}", other)),
    }
}

pub async fn listen_for_messages(client: &IMClient) -> ! {
    listen_for_messages_with_logger(client, None, None).await
}

pub async fn listen_for_messages_with_logger(
    client: &IMClient,
    logger: Option<&MessageLogger>,
    download_dir: Option<PathBuf>,
) -> ! {
    println!("Listening for incoming messages... (Ctrl+C to exit)");

    loop {
        let mut receiver = client.conn.subscribe().await;
        loop {
            match receiver.recv().await {
                Ok(msg) => match client.handle(msg).await {
                    Ok(Some(message)) => {
                        let has_payload = message.has_payload();
                        if let Some(timing) = message.timing_info.as_ref() {
                            let delta = timing
                                .ts_measured_received
                                .saturating_sub(timing.ts_measured_after_send);
                            //info!(
                            //    "received message response from token {} after {} milliseconds",
                            //    timing.token, delta
                            //);
                        }

                        if let Some(logger) = logger {
                            let raw_xml = extract_raw_xml(client, &message, has_payload).await;
                            let _ = raw_xml; // raw payload no longer stored
                            logger.log_incoming(&message).await;
                        }

                        print_message(&message);
                        if let Err(err) = log_message_raw(client, &message, has_payload).await {
                            eprintln!("Failed to build message.to_raw output: {err}");
                        }
                        if let Some(dir) = download_dir.as_deref() {
                            if let Err(err) =
                                download_message_attachments(client, &message, dir).await
                            {
                                eprintln!("Failed to download attachments: {err}");
                            }
                        }
                    }
                    Ok(None) => {}
                    Err(err) => {
                        eprintln!("Failed to handle APS message: {err:?}");
                    }
                },
                Err(RecvError::Closed) => {
                    eprintln!("APS connection closed; attempting to resubscribe.");
                    break;
                }
                Err(RecvError::Lagged(skipped)) => {
                    eprintln!("Missed {skipped} APS messages; continuing.");
                }
            }
        }

        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}

fn sanitize_component(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '_') {
                ch
            } else {
                '_'
            }
        })
        .collect()
}

fn sanitize_filename(value: &str) -> Option<String> {
    let name = Path::new(value).file_name()?.to_string_lossy().to_string();
    if name.trim().is_empty() {
        None
    } else {
        Some(name)
    }
}

async fn download_attachments_from_parts(
    client: &IMClient,
    message_id: &str,
    parts: &MessageParts,
    download_dir: &Path,
) -> Result<(), String> {
    if !parts.has_attachments() {
        return Ok(());
    }

    fs::create_dir_all(download_dir).map_err(|err| err.to_string())?;
    let apns_resource = client.conn.resource.clone();

    let mut idx = 0usize;
    for part in &parts.0 {
        let MessagePart::Attachment(attachment) = &part.part else {
            continue;
        };
        let base = sanitize_filename(&attachment.name)
            .unwrap_or_else(|| format!("attachment_{idx}.bin"));
        let name = format!(
            "{}_{}_{}",
            sanitize_component(message_id),
            idx,
            sanitize_component(&base)
        );
        let path = download_dir.join(name);
        let file = fs::File::create(&path).map_err(|err| err.to_string())?;
        attachment
            .get_attachment(apns_resource.as_ref(), file, |_progress, _total| {})
            .await
            .map_err(|err| err.to_string())?;
        println!("Downloaded attachment to {}", path.display());
        idx += 1;
    }

    Ok(())
}

async fn download_message_attachments(
    client: &IMClient,
    message: &MessageInst,
    download_dir: &Path,
) -> Result<(), String> {
    match &message.message {
        Message::Message(msg) => {
            download_attachments_from_parts(client, &message.id, &msg.parts, download_dir).await
        }
        Message::React(react) => {
            if let ReactMessageType::React {
                reaction: Reaction::Sticker { body, .. },
                ..
            } = &react.reaction
            {
                download_attachments_from_parts(client, &message.id, body, download_dir).await
            } else {
                Ok(())
            }
        }
        _ => Ok(()),
    }
}

pub async fn listen_for_messages_for(client: &IMClient, duration: Duration) {
    println!(
        "Listening for incoming messages for {} seconds... (Ctrl+C to exit early)",
        duration.as_secs()
    );

    let mut receiver = client.conn.subscribe().await;
    let deadline = Instant::now() + duration;

    loop {
        tokio::select! {
            _ = tokio::time::sleep_until(deadline) => {
                println!("Finished listening after {} seconds.", duration.as_secs());
                break;
            }
            result = receiver.recv() => {
                match result {
                    Ok(msg) => match client.handle(msg).await {
                        Ok(Some(message)) => {
                            let has_payload = message.has_payload();
                            if let Some(timing) = message.timing_info.as_ref() {
                                let delta = timing
                                    .ts_measured_received
                                    .saturating_sub(timing.ts_measured_after_send);
                                info!(
                                    "received message response from token {} after {} milliseconds",
                                    timing.token, delta
                                );
                            }

                            print_message(&message);
                            if let Err(err) = log_message_raw(client, &message, has_payload).await {
                                eprintln!("Failed to build message.to_raw output: {err}");
                            }
                        }
                        Ok(None) => {}
                        Err(err) => {
                            eprintln!("Failed to handle APS message: {err:?}");
                        }
                    },
                    Err(RecvError::Closed) => {
                        eprintln!("APS connection closed; stopping timed listener.");
                        break;
                    }
                    Err(RecvError::Lagged(skipped)) => {
                        eprintln!("Missed {skipped} APS messages; continuing.");
                    }
                }
            }
        }
    }
}

pub const DEFAULT_IPC_SOCKET: &str = "/tmp/rustpush.sock";

fn communication_partner(message: &MessageInst) -> Option<String> {
    let sender = message.sender.as_deref();
    message.conversation.as_ref().and_then(|conv| {
        conv.participants
            .iter()
            .find(|p| Some(p.as_str()) != sender)
            .or_else(|| conv.participants.first())
            .cloned()
    })
}

#[derive(Serialize, Deserialize)]
pub enum IpcRequest {
    SendMessage {
        target: String,
        text: String,
        xml: Option<String>,
        push_token: Option<String>,
    },
    SendAttachment {
        target: String,
        file_path: String,
        text: Option<String>,
        name: Option<String>,
        mime: Option<String>,
        uti: Option<String>,
        inline: bool,
        extra_participants: Option<String>,
        push_token: Option<String>,
    },
    SendTyping {
        target: String,
        typing: bool,
        push_token: Option<String>,
        send_delivered: Option<bool>,
    },
    TestMessageTypes {
        target: String,
        typing: bool,
        push_token: Option<String>,
        message_index: Option<usize>,
    },
    SendReaction {
        target: String,
        guid: String,
        reaction: String,
        enable: bool,
        text: Option<String>,
        t_value: Option<String>,
        amt: Option<u64>,
        part_index: u64,
        extra_participants: Option<String>,
        emoji: Option<String>,
        push_token: Option<String>,
    },
}

#[derive(Serialize, Deserialize)]
pub struct IpcResponse {
    pub ok: bool,
    pub error: Option<String>,
}

#[derive(Clone)]
pub struct MessageLogger {
    db_path: PathBuf,
}

impl MessageLogger {
    pub fn new(db_path: PathBuf) -> Result<Self, String> {
        Self::init_db(&db_path)?;
        Ok(Self { db_path })
    }

    fn init_db(path: &Path) -> Result<(), String> {
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
        }
        let conn = Connection::open(path).map_err(|e| e.to_string())?;
        conn.execute("DROP TABLE IF EXISTS outgoing_messages", [])
            .map_err(|e| e.to_string())?;
        conn.execute("DROP TABLE IF EXISTS incoming_messages", [])
            .map_err(|e| e.to_string())?;
        conn.execute(
            "CREATE TABLE IF NOT EXISTS incoming_messages (
                id TEXT,
                sender TEXT,
                partner TEXT,
                token TEXT,
                delta_ms INTEGER,
                ts_before_send INTEGER,
                ts_after_send INTEGER,
                ts_received INTEGER,
                ts_server INTEGER
            )",
            [],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub async fn log_incoming(&self, message: &MessageInst) {
        let Some(timing) = message.timing_info.as_ref() else {
            return;
        };

        let id = message.id.clone();
        let sender = message
            .sender
            .clone()
            .unwrap_or_else(|| "unknown".to_string());
        let partner = communication_partner(message).unwrap_or_else(|| "unknown".to_string());
        let token = timing.token.clone();
        let delta_ms = timing
            .ts_measured_received
            .saturating_sub(timing.ts_measured_after_send);
        let ts_before_send = timing.ts_meausred_before_send as i64;
        let ts_after_send = timing.ts_measured_after_send as i64;
        let ts_received = timing.ts_measured_received as i64;
        let ts_server = timing.ts_server as i64;
        let db_path = self.db_path.clone();

        if let Err(err) = tokio::task::spawn_blocking(move || {
            let conn = Connection::open(db_path).map_err(|e| e.to_string())?;
            conn.execute(
                "INSERT INTO incoming_messages (id, sender, partner, token, delta_ms, ts_before_send, ts_after_send, ts_received, ts_server)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                (
                    id,
                    sender,
                    partner,
                    token,
                    delta_ms as i64,
                    ts_before_send,
                    ts_after_send,
                    ts_received,
                    ts_server,
                ),
            )
            .map_err(|e| e.to_string())
        })
        .await
        .map_err(|e| e.to_string())
        .and_then(|res| res)
        {
            eprintln!("Failed to log timing info to SQLite: {err}");
        }
    }

    pub async fn log_outgoing(
        &self,
        message: &MessageInst,
        raw_xml: Option<String>,
        command: Option<u8>,
        no_response: Option<bool>,
        extras: Option<String>,
        scheduled_ms: Option<i64>,
        queue_id: Option<String>,
        relay: Option<String>,
        tokens: Vec<(String, String)>,
    ) {
        let _ = (
            message,
            raw_xml,
            command,
            no_response,
            extras,
            scheduled_ms,
            queue_id,
            relay,
            tokens,
        );
        // Outgoing messages are no longer logged.
    }
}

pub async fn send_typing_indicator(
    client: &IMClient,
    target: &str,
    typing: bool,
    push_token: Option<Vec<u8>>,
    send_delivered: bool,
) -> Result<(), String> {
    let handles = client.identity.get_handles().await;
    let Some(sender_handle) = handles.first() else {
        return Err("No sender handles available".to_string());
    };

    let mut message_inst = MessageInst::new_with_send_delivered(
        build_conversation(target),
        sender_handle,
        rustpush::Message::Typing(typing, None),
        Some(send_delivered),
    );

    if let Some(token) = push_token {
        if let Some(conv) = message_inst.conversation.as_mut() {
            conv.participants = vec![target.to_string()];
        }
        message_inst.target = Some(vec![MessageTarget::Token(token)]);
    }

    client
        .send(&mut message_inst)
        .await
        .map(|_| ())
        .map_err(|err| err.to_string())
}

pub async fn send_text_message_with_logging(
    client: &IMClient,
    target: &str,
    text: &str,
    xml: Option<String>,
    logger: Option<&MessageLogger>,
    push_token: Option<Vec<u8>>,
) -> Result<(), String> {
    let handles = client.identity.get_handles().await;
    let Some(sender_handle) = handles.first() else {
        return Err("No sender handles available".to_string());
    };

    let mut msg = NormalMessage::new(text.to_string(), MessageType::IMessage);
    msg.xml = xml;
    let mut message_inst = MessageInst::new(
        build_conversation(target),
        sender_handle,
        Message::Message(msg),
    );

    if let Some(token) = push_token.clone() {
        if let Some(conv) = message_inst.conversation.as_mut() {
            conv.participants = vec![target.to_string()];
        }
        message_inst.target = Some(vec![MessageTarget::Token(token)]);
    }

    if let Some(logger) = logger {
        let raw_xml = extract_raw_xml(client, &message_inst, message_inst.has_payload()).await;
        let mut tokens = extract_tokens(client, &message_inst).await;
        if let Some(token) = push_token {
            let needle = encode_hex(&token);
            tokens.retain(|(pt, _)| *pt == needle);
            if tokens.is_empty() {
                tokens.push((needle, String::new()));
            }
        }
        logger
            .log_outgoing(
                &message_inst,
                raw_xml,
                Some(message_inst.message.get_c()),
                message_inst.message.get_nr(),
                serialize_extras(&message_inst.message),
                message_inst.message.ids_scheduled_ms().map(|v| v as i64),
                if message_inst.is_queued() {
                    Some(message_inst.queue_id())
                } else {
                    None
                },
                None,
                tokens,
            )
            .await;
    }

    client
        .send(&mut message_inst)
        .await
        .map(|_| ())
        .map_err(|err| err.to_string())
}

fn guess_mime_uti(path: &Path) -> (String, String) {
    let ext = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .to_lowercase();
    let (mime, uti) = match ext.as_str() {
        "png" => ("image/png", "public.png"),
        "jpg" | "jpeg" => ("image/jpeg", "public.jpeg"),
        "gif" => ("image/gif", "com.compuserve.gif"),
        "heic" => ("image/heic", "public.heic"),
        "heif" => ("image/heif", "public.heif"),
        "mp4" => ("video/mp4", "public.mpeg-4"),
        "mov" => ("video/quicktime", "com.apple.quicktime-movie"),
        "pdf" => ("application/pdf", "com.adobe.pdf"),
        "txt" => ("text/plain", "public.plain-text"),
        "json" => ("application/json", "public.json"),
        "zip" => ("application/zip", "public.zip-archive"),
        "gz" => ("application/gzip", "org.gnu.gnu-zip-archive"),
        "tar" => ("application/x-tar", "public.tar-archive"),
        _ => ("application/octet-stream", "public.data"),
    };
    (mime.to_string(), uti.to_string())
}

fn attachment_name(path: &Path, override_name: Option<&str>) -> Result<String, String> {
    if let Some(name) = override_name {
        return Ok(name.to_string());
    }
    let file_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| "Unable to determine attachment filename".to_string())?;
    Ok(file_name.to_string())
}

pub async fn send_attachment_message(
    client: &IMClient,
    target_handle: &str,
    file_path: &Path,
    text: Option<String>,
    name_override: Option<String>,
    mime_override: Option<String>,
    uti_override: Option<String>,
    inline: bool,
    participants: Vec<String>,
    logger: Option<&MessageLogger>,
    push_token: Option<Vec<u8>>,
) -> Result<(), String> {
    let handles = client.identity.get_handles().await;
    let Some(sender_handle) = handles.first() else {
        return Err("No sender handles available".to_string());
    };

    if !file_path.exists() {
        return Err(format!(
            "Attachment file does not exist: {}",
            file_path.display()
        ));
    }

    let name = attachment_name(file_path, name_override.as_deref())?;
    let (default_mime, default_uti) = guess_mime_uti(file_path);
    let mime = mime_override.unwrap_or(default_mime);
    let uti = uti_override.unwrap_or(default_uti);

    let attachment = if inline {
        let data = fs::read(file_path).map_err(|err| err.to_string())?;
        Attachment {
            a_type: AttachmentType::Inline(data),
            part: 0,
            uti_type: uti,
            mime,
            name,
            iris: false,
        }
    } else {
        let apns_resource = client.conn.resource.clone();
        let prepare_file =
            fs::File::open(file_path).map_err(|err| format!("Failed to open file: {err}"))?;
        let prepared =
            MMCSFile::prepare_put(prepare_file).await.map_err(|err| err.to_string())?;
        let file =
            fs::File::open(file_path).map_err(|err| format!("Failed to open file: {err}"))?;
        Attachment::new_mmcs(
            apns_resource.as_ref(),
            &prepared,
            file,
            &mime,
            &uti,
            &name,
            |_progress, _total| {},
        )
        .await
        .map_err(|err| err.to_string())?
    };

    let mut parts = Vec::new();
    if let Some(text) = text {
        if !text.is_empty() {
            parts.push(IndexedMessagePart {
                part: MessagePart::Text(text, TextFormat::default()),
                idx: None,
                ext: None,
            });
        }
    }

    parts.push(IndexedMessagePart {
        part: MessagePart::Attachment(attachment),
        idx: None,
        ext: None,
    });

    let message = NormalMessage {
        parts: MessageParts(parts),
        effect: None,
        reply_guid: None,
        reply_part: None,
        service: MessageType::IMessage,
        xml: None,
        subject: None,
        app: None,
        link_meta: None,
        voice: false,
        scheduled: None,
        embedded_profile: None,
    };

    let mut message_inst = MessageInst::new(
        ConversationData {
            participants,
            cv_name: None,
            sender_guid: None,
            after_guid: None,
        },
        sender_handle,
        Message::Message(message),
    );

    if let Some(token) = push_token.clone() {
        if let Some(conv) = message_inst.conversation.as_mut() {
            conv.participants = vec![target_handle.to_string()];
        }
        message_inst.target = Some(vec![MessageTarget::Token(token)]);
    }

    if let Some(logger) = logger {
        let raw_xml = extract_raw_xml(client, &message_inst, message_inst.has_payload()).await;
        let mut tokens = extract_tokens(client, &message_inst).await;
        if let Some(token) = push_token {
            let needle = encode_hex(&token);
            tokens.retain(|(pt, _)| *pt == needle);
            if tokens.is_empty() {
                tokens.push((needle, String::new()));
            }
        }
        logger
            .log_outgoing(
                &message_inst,
                raw_xml,
                Some(message_inst.message.get_c()),
                message_inst.message.get_nr(),
                serialize_extras(&message_inst.message),
                message_inst.message.ids_scheduled_ms().map(|v| v as i64),
                if message_inst.is_queued() {
                    Some(message_inst.queue_id())
                } else {
                    None
                },
                None,
                tokens,
            )
            .await;
    }

    client
        .send(&mut message_inst)
        .await
        .map(|_| ())
        .map_err(|err| err.to_string())
}

pub async fn send_text_message_with_logging_cached(
    client: &IMClient,
    target: &str,
    text: &str,
    xml: Option<String>,
    logger: Option<&MessageLogger>,
    push_token: Option<Vec<u8>>,
) -> Result<(), String> {
    let handles = client.identity.get_handles().await;
    let Some(sender_handle) = handles.first() else {
        return Err("No sender handles available".to_string());
    };

    let mut msg = NormalMessage::new(text.to_string(), MessageType::IMessage);
    msg.xml = xml;
    let mut message_inst = MessageInst::new(
        build_conversation(target),
        sender_handle,
        Message::Message(msg),
    );

    if let Some(token) = push_token.clone() {
        if let Some(conv) = message_inst.conversation.as_mut() {
            conv.participants = vec![target.to_string()];
        }
        message_inst.target = Some(vec![MessageTarget::Token(token)]);
    }

    if let Some(logger) = logger {
        let raw_xml = extract_raw_xml(client, &message_inst, message_inst.has_payload()).await;
        let mut tokens = extract_tokens(client, &message_inst).await;
        if let Some(token) = push_token {
            let needle = encode_hex(&token);
            tokens.retain(|(pt, _)| *pt == needle);
            if tokens.is_empty() {
                tokens.push((needle, String::new()));
            }
        }
        logger
            .log_outgoing(
                &message_inst,
                raw_xml,
                Some(message_inst.message.get_c()),
                message_inst.message.get_nr(),
                serialize_extras(&message_inst.message),
                message_inst.message.ids_scheduled_ms().map(|v| v as i64),
                if message_inst.is_queued() {
                    Some(message_inst.queue_id())
                } else {
                    None
                },
                None,
                tokens,
            )
            .await;
    }

    client
        .send_using_cached_targets(&mut message_inst)
        .await
        .map(|_| ())
        .map_err(|err| err.to_string())
}

pub async fn test_message_types(
    client: &IMClient,
    target: &str,
    typing: bool,
    logger: Option<&MessageLogger>,
    push_token: Option<Vec<u8>>,
    message_index: Option<usize>,
) -> Result<(), String> {
    let handles = client.identity.get_handles().await;
    let Some(sender_handle) = handles.first() else {
        return Err("No sender handles available".to_string());
    };
    let mut test_messages = vec![
        rustpush::Message::Typing(typing, None),
        rustpush::Message::Delivered,
        rustpush::Message::Read,
        rustpush::Message::MarkUnread,
        rustpush::Message::RenameMessage(RenameMessage {
            new_name: String::new(),
        }),
        rustpush::Message::Unsend(UnsendMessage {
            tuuid: String::new(),
            edit_part: 0,
        }),
        rustpush::Message::Edit(EditMessage {
            tuuid: String::new(),
            edit_part: 0,
            new_parts: MessageParts(vec![IndexedMessagePart {
                part: MessagePart::Text(String::new(), TextFormat::default()),
                idx: None,
                ext: None,
            }]),
        }),
        rustpush::Message::IconChange(IconChangeMessage {
            file: None,
            group_version: 0,
        }),
        rustpush::Message::MessageReadOnDevice,
        rustpush::Message::PeerCacheInvalidate,
        rustpush::Message::UpdateExtension(UpdateExtensionMessage {
            for_uuid: String::new(),
            ext: PartExtension::Sticker {
                msg_width: 0.0,
                rotation: 0.0,
                sai: 0,
                scale: 0.0,
                update: None,
                sli: 0,
                normalized_x: 0.0,
                normalized_y: 0.0,
                version: 0,
                hash: String::new(),
                safi: 0,
                effect_type: 0,
                sticker_id: String::new(),
            },
        }),
        rustpush::Message::Error(ErrorMessage {
            for_uuid: String::new(),
            status: 0,
            status_str: String::new(),
            token: None,
        }),
        rustpush::Message::MoveToRecycleBin(MoveToRecycleBinMessage {
            target: DeleteTarget::Messages(Vec::new()),
            recoverable_delete_date: 0,
        }),
        rustpush::Message::RecoverChat(OperatedChat {
            participants: Vec::new(),
            group_id: String::new(),
            guid: String::new(),
            delete_incoming_messages: None,
            was_reported_as_junk: None,
        }),
        rustpush::Message::PermanentDelete(PermanentDeleteMessage {
            target: DeleteTarget::Messages(Vec::new()),
            is_scheduled: false,
        }),
        rustpush::Message::Unschedule,
        rustpush::Message::UpdateProfile(UpdateProfileMessage {
            profile: None,
            share_contacts: false,
        }),
        rustpush::Message::UpdateProfileSharing(UpdateProfileSharingMessage {
            shared_dismissed: Vec::new(),
            shared_all: Vec::new(),
            version: 0,
        }),
        rustpush::Message::ShareProfile(ShareProfileMessage {
            cloud_kit_decryption_record_key: Vec::new(),
            cloud_kit_record_key: String::new(),
            poster: None,
        }),
        rustpush::Message::NotifyAnyways,
        rustpush::Message::SetTranscriptBackground(SetTranscriptBackgroundMessage::Remove {
            aid: 0,
            bid: 0,
            chat_id: None,
            remove: true,
        }),
    ];
    let selected_messages = if let Some(index) = message_index {
        if index == 0 || index > test_messages.len() {
            return Err(format!(
                "Message index {index} out of range (1..={})",
                test_messages.len()
            ));
        }
        vec![test_messages.remove(index - 1)]
    } else {
        test_messages
    };
    for message in selected_messages {
        println!("Sending message type: {}", describe_message_type(&message));
        let mut message_inst = MessageInst::new_with_send_delivered(
            build_conversation(target),
            sender_handle,
            message,
            Some(true),
        );

        if let Some(token) = push_token.clone() {
            if let Some(conv) = message_inst.conversation.as_mut() {
                conv.participants = vec![target.to_string()];
            }
            message_inst.target = Some(vec![MessageTarget::Token(token)]);
        }

        if let Some(logger) = logger {
            let raw_xml = extract_raw_xml(client, &message_inst, message_inst.has_payload()).await;
            let mut tokens = extract_tokens(client, &message_inst).await;
            if let Some(token) = push_token.clone() {
                let needle = encode_hex(&token);
                tokens.retain(|(pt, _)| *pt == needle);
                if tokens.is_empty() {
                    tokens.push((needle, String::new()));
                }
            }
            logger
                .log_outgoing(
                    &message_inst,
                    raw_xml,
                    Some(message_inst.message.get_c()),
                    message_inst.message.get_nr(),
                    serialize_extras(&message_inst.message),
                    message_inst.message.ids_scheduled_ms().map(|v| v as i64),
                    if message_inst.is_queued() {
                        Some(message_inst.queue_id())
                    } else {
                        None
                    },
                    None,
                    tokens,
                )
                .await;
        }

        client
            .send(&mut message_inst)
            .await
            .map(|_| ())
            .map_err(|err| err.to_string())?;

        tokio::time::sleep(Duration::from_secs(10)).await;
    }
    Ok(())
}

pub async fn send_typing_indicator_with_logging(
    client: &IMClient,
    target: &str,
    typing: bool,
    logger: Option<&MessageLogger>,
    push_token: Option<Vec<u8>>,
    send_delivered: bool,
) -> Result<(), String> {
    let handles = client.identity.get_handles().await;
    let Some(sender_handle) = handles.first() else {
        return Err("No sender handles available".to_string());
    };

    let mut message_inst = MessageInst::new_with_send_delivered(
        build_conversation(target),
        sender_handle,
        rustpush::Message::Typing(typing, None),
        Some(send_delivered),
    );

    if let Some(token) = push_token.clone() {
        if let Some(conv) = message_inst.conversation.as_mut() {
            conv.participants = vec![target.to_string()];
        }
        message_inst.target = Some(vec![MessageTarget::Token(token)]);
    }

    if let Some(logger) = logger {
        let raw_xml = extract_raw_xml(client, &message_inst, message_inst.has_payload()).await;
        let mut tokens = extract_tokens(client, &message_inst).await;
        if let Some(token) = push_token {
            let needle = encode_hex(&token);
            tokens.retain(|(pt, _)| *pt == needle);
            if tokens.is_empty() {
                tokens.push((needle, String::new()));
            }
        }
        logger
            .log_outgoing(
                &message_inst,
                raw_xml,
                Some(message_inst.message.get_c()),
                message_inst.message.get_nr(),
                serialize_extras(&message_inst.message),
                message_inst.message.ids_scheduled_ms().map(|v| v as i64),
                if message_inst.is_queued() {
                    Some(message_inst.queue_id())
                } else {
                    None
                },
                None,
                tokens,
            )
            .await;
    }

    client
        .send(&mut message_inst)
        .await
        .map(|_| ())
        .map_err(|err| err.to_string())
}

pub async fn send_reaction_message(
    client: &IMClient,
    target_handle: &str,
    target_guid: &str,
    reaction: Reaction,
    enable: bool,
    original_text: String,
    custom_t: Option<String>,
    custom_amt: Option<u64>,
    part_index: u64,
    participants: Vec<String>,
    logger: Option<&MessageLogger>,
    push_token: Option<Vec<u8>>,
) -> Result<(), String> {
    let handles = client.identity.get_handles().await;
    let Some(sender_handle) = handles.first() else {
        return Err("No sender handles available".to_string());
    };

    let mut message_inst = MessageInst::new_with_send_delivered(
        ConversationData {
            participants,
            cv_name: None,
            sender_guid: None,
            after_guid: None,
        },
        sender_handle,
        Message::React(ReactMessage {
            to_uuid: target_guid.to_string(),
            to_part: Some(part_index),
            reaction: ReactMessageType::React { reaction, enable },
            to_text: original_text,
            custom_text: custom_t,
            custom_amt,
            embedded_profile: None,
        }),
        Some(true),
    );

    if let Some(token) = push_token.clone() {
        if let Some(conv) = message_inst.conversation.as_mut() {
            conv.participants = vec![target_handle.to_string()];
        }
        message_inst.target = Some(vec![MessageTarget::Token(token)]);
    }

    if let Some(logger) = logger {
        let raw_xml = extract_raw_xml(client, &message_inst, message_inst.has_payload()).await;
        let mut tokens = extract_tokens(client, &message_inst).await;
        if let Some(token) = push_token {
            let needle = encode_hex(&token);
            tokens.retain(|(pt, _)| *pt == needle);
            if tokens.is_empty() {
                tokens.push((needle, String::new()));
            }
        }
        logger
            .log_outgoing(
                &message_inst,
                raw_xml,
                Some(message_inst.message.get_c()),
                message_inst.message.get_nr(),
                serialize_extras(&message_inst.message),
                message_inst.message.ids_scheduled_ms().map(|v| v as i64),
                if message_inst.is_queued() {
                    Some(message_inst.queue_id())
                } else {
                    None
                },
                None,
                tokens,
            )
            .await;
    }

    client
        .send(&mut message_inst)
        .await
        .map(|_| ())
        .map_err(|err| err.to_string())
}

pub async fn spawn_ipc_server(
    client: Arc<IMClient>,
    socket_path: &Path,
    logger: Option<Arc<MessageLogger>>,
) -> io::Result<JoinHandle<()>> {
    if socket_path.exists() {
        let _ = std::fs::remove_file(socket_path);
    }

    let listener = UnixListener::bind(socket_path)?;
    let socket_path = socket_path.to_path_buf();

    Ok(tokio::spawn(async move {
        loop {
            match listener.accept().await {
                Ok((stream, _)) => {
                    let client = client.clone();
                    let logger = logger.clone();
                    tokio::spawn(async move {
                        if let Err(err) = handle_ipc_connection(stream, client, logger).await {
                            eprintln!("IPC client error: {err}");
                        }
                    });
                }
                Err(err) => {
                    eprintln!("IPC accept error: {err}");
                    break;
                }
            }
        }

        let _ = tokio::fs::remove_file(&socket_path).await;
    }))
}

pub async fn send_ipc_request(socket_path: &Path, request: &IpcRequest) -> io::Result<IpcResponse> {
    let mut stream = UnixStream::connect(socket_path).await?;
    let payload = serde_json::to_vec(request)
        .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
    stream.write_all(&payload).await?;
    stream.shutdown().await?;

    let mut buf = Vec::new();
    stream.read_to_end(&mut buf).await?;

    serde_json::from_slice(&buf).map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))
}

async fn handle_ipc_connection(
    mut stream: UnixStream,
    client: Arc<IMClient>,
    logger: Option<Arc<MessageLogger>>,
) -> io::Result<()> {
    let mut buf = Vec::new();
    stream.read_to_end(&mut buf).await?;

    let request: IpcRequest = serde_json::from_slice(&buf)
        .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;

    let response = match request {
        IpcRequest::SendMessage {
            target,
            text,
            xml,
            push_token,
        } => match send_text_message_with_logging(
            client.as_ref(),
            &target,
            &text,
            xml,
            logger.as_deref(),
            push_token.and_then(|hex| decode_hex(&hex).ok()),
        )
        .await
        {
            Ok(()) => IpcResponse {
                ok: true,
                error: None,
            },
            Err(err) => IpcResponse {
                ok: false,
                error: Some(err),
            },
        },
        IpcRequest::SendAttachment {
            target,
            file_path,
            text,
            name,
            mime,
            uti,
            inline,
            extra_participants,
            push_token,
        } => {
            let push_token = push_token
                .map(|hex| decode_hex(&hex).map_err(|err| format!("Invalid push token hex: {err}")))
                .transpose();

            match push_token {
                Ok(push_token) => match send_attachment_message(
                    client.as_ref(),
                    &target,
                    Path::new(&file_path),
                    text,
                    name,
                    mime,
                    uti,
                    inline,
                    parse_participants(&target, extra_participants.as_deref()),
                    logger.as_deref(),
                    push_token,
                )
                .await
                {
                    Ok(()) => IpcResponse {
                        ok: true,
                        error: None,
                    },
                    Err(err) => IpcResponse {
                        ok: false,
                        error: Some(err),
                    },
                },
                Err(err) => IpcResponse {
                    ok: false,
                    error: Some(err),
                },
            }
        }
        IpcRequest::SendTyping {
            target,
            typing,
            push_token,
            send_delivered,
        } => match send_typing_indicator_with_logging(
            client.as_ref(),
            &target,
            typing,
            logger.as_deref(),
            push_token.and_then(|hex| decode_hex(&hex).ok()),
            send_delivered.unwrap_or(true),
        )
        .await
        {
            Ok(()) => IpcResponse {
                ok: true,
                error: None,
            },
            Err(err) => IpcResponse {
                ok: false,
                error: Some(err),
            },
        },
        IpcRequest::TestMessageTypes {
            target,
            typing,
            push_token,
            message_index,
        } => match test_message_types(
            client.as_ref(),
            &target,
            typing,
            logger.as_deref(),
            push_token.and_then(|hex| decode_hex(&hex).ok()),
            message_index,
        )
        .await
        {
            Ok(()) => IpcResponse {
                ok: true,
                error: None,
            },
            Err(err) => IpcResponse {
                ok: false,
                error: Some(err),
            },
        },
        IpcRequest::SendReaction {
            target,
            guid,
            reaction,
            enable,
            text,
            t_value,
            amt,
            part_index,
            extra_participants,
            emoji,
            push_token,
        } => {
            let push_token = push_token
                .map(|hex| decode_hex(&hex).map_err(|err| format!("Invalid push token hex: {err}")))
                .transpose();

            match (parse_reaction(&reaction, emoji.as_deref()), push_token) {
                (Ok(reaction), Ok(push_token)) => {
                    match send_reaction_message(
                        client.as_ref(),
                        &target,
                        &guid,
                        reaction,
                        enable,
                        text.unwrap_or_default(),
                        t_value,
                        amt,
                        part_index,
                        parse_participants(&target, extra_participants.as_deref()),
                        logger.as_deref(),
                        push_token,
                    )
                    .await
                    {
                        Ok(()) => IpcResponse {
                            ok: true,
                            error: None,
                        },
                        Err(err) => IpcResponse {
                            ok: false,
                            error: Some(err),
                        },
                    }
                }
                (Err(err), _) => IpcResponse {
                    ok: false,
                    error: Some(err),
                },
                (_, Err(err)) => IpcResponse {
                    ok: false,
                    error: Some(err),
                },
            }
        }
    };

    let response_bytes = serde_json::to_vec(&response)
        .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
    stream.write_all(&response_bytes).await?;
    Ok(())
}

fn build_conversation(target: &str) -> ConversationData {
    ConversationData {
        participants: vec![target.to_string()],
        cv_name: None,
        sender_guid: None,
        after_guid: None,
    }
}

fn serialize_extras(message: &rustpush::Message) -> Option<String> {
    let extras = message.extras();
    if extras.is_empty() {
        return None;
    }
    let dict = Value::Dictionary(extras);
    serde_json::to_string(&dict)
        .ok()
        .or_else(|| plist_to_string(&dict).ok())
}

async fn extract_raw_xml(
    client: &IMClient,
    message: &MessageInst,
    has_payload: bool,
) -> Option<String> {
    if !has_payload {
        return Some("No body".to_string());
    }

    let Some(sender_handle) = message.sender.as_deref() else {
        return None;
    };

    let conversation = message
        .conversation
        .as_ref()
        .map(|conv| ConversationData {
            participants: conv.participants.clone(),
            cv_name: conv.cv_name.clone(),
            sender_guid: conv.sender_guid.clone(),
            after_guid: conv.after_guid.clone(),
        })
        .or_else(|| {
            Some(ConversationData {
                participants: vec![sender_handle.to_string()],
                cv_name: None,
                sender_guid: None,
                after_guid: None,
            })
        })?;

    let handles = client.identity.get_handles().await;
    let apns_resource = client.conn.resource.clone();

    let mut temp = MessageInst::new_with_send_delivered(
        conversation,
        sender_handle,
        message.message.clone(),
        Some(message.send_delivered),
    );
    temp.id = message.id.clone();
    temp.target = message.target.clone();

    let Ok(raw_bytes) = temp.to_raw(&handles, apns_resource.as_ref(), false).await else {
        return None;
    };

    bytes_to_xml(&raw_bytes)
}

fn bytes_to_xml(bytes: &[u8]) -> Option<String> {
    if let Ok(value) = Value::from_reader(Cursor::new(bytes)) {
        if let Ok(xml) = plist_to_string(&value) {
            return Some(xml);
        }
    }

    if let Ok(unzipped) = ungzip(bytes) {
        if let Ok(value) = Value::from_reader(Cursor::new(unzipped)) {
            if let Ok(xml) = plist_to_string(&value) {
                return Some(xml);
            }
        }
    }

    None
}

async fn extract_tokens(client: &IMClient, message: &MessageInst) -> Vec<(String, String)> {
    let handles = client.identity.get_handles().await;
    let topic = if message.message.is_sms() {
        "com.apple.private.alloy.sms"
    } else {
        "com.apple.madrid"
    };

    let mut temp = message.clone();
    let targets = temp.prepare_send(&handles);
    if targets.is_empty() {
        return Vec::new();
    }

    let handle = message.sender.as_ref().unwrap().to_string();
    let ident_cache = client.identity.cache.lock().await;
    let message_targets = if let Some(message_targets) = &message.target {
        match ident_cache.get_targets(topic, &handle, &targets, message_targets) {
            Ok(t) => t,
            Err(err) => {
                eprintln!("Failed to get message targets for token logging: {err}");
                return Vec::new();
            }
        }
    } else {
        ident_cache.get_participants_targets(topic, &handle, &targets)
    };
    drop(ident_cache);

    message_targets
        .into_iter()
        .map(|t| {
            (
                encode_hex(&t.delivery_data.push_token),
                encode_hex(&t.delivery_data.session_token),
            )
        })
        .collect()
}

fn print_message(message: &MessageInst) {
    println!("Received message: {}", message.message);
    println!(
        "  meta id={} sent_timestamp={} sender={} type={}",
        message.id,
        message.sent_timestamp,
        message.sender.as_deref().unwrap_or("unknown"),
        describe_message_type(&message.message)
    );
    let sender = message.sender.as_deref().unwrap_or("unknown");
    let participants = message
        .conversation
        .as_ref()
        .and_then(|conv| {
            if conv.participants.is_empty() {
                None
            } else {
                Some(conv.participants.join(", "))
            }
        })
        .or_else(|| message.sender.clone())
        .unwrap_or_else(|| "unknown participants".to_string());
    let timestamp = if message.sent_timestamp != 0 {
        message.sent_timestamp.to_string()
    } else {
        "-".to_string()
    };

    println!(
        "[{timestamp}] sender={sender} participants=[{participants}] -> {}",
        message.message
    );
    if let Message::Error(err) = &message.message {
        if let Some(token) = err.token.as_deref() {
            println!("  error_token={token}");
        }
    }
}

fn describe_message_type(message: &rustpush::Message) -> &'static str {
    use rustpush::Message::*;
    match message {
        Message(_) => "text",
        RenameMessage(_) => "rename",
        ChangeParticipants(_) => "change_participants",
        React(_) => "reaction",
        Delivered => "delivered",
        Read => "read",
        Typing(_, _) => "typing",
        Unsend(_) => "unsend",
        Edit(_) => "edit",
        IconChange(_) => "icon_change",
        EnableSmsActivation(_) => "enable_sms_activation",
        MessageReadOnDevice => "message_read_on_device",
        SmsConfirmSent(_) => "sms_confirm_sent",
        MarkUnread => "mark_unread",
        PeerCacheInvalidate => "peer_cache_invalidate",
        UpdateExtension(_) => "update_extension",
        Error(_) => "error",
        Ack(_) => "ack",
        MoveToRecycleBin(_) => "move_to_recycle_bin",
        RecoverChat(_) => "recover_chat",
        PermanentDelete(_) => "permanent_delete",
        Unschedule => "unschedule",
        UpdateProfile(_) => "update_profile",
        UpdateProfileSharing(_) => "update_profile_sharing",
        ShareProfile(_) => "share_profile",
        NotifyAnyways => "notify_anyways",
        SetTranscriptBackground(_) => "set_transcript_background",
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

async fn log_message_raw(
    client: &IMClient,
    message: &MessageInst,
    has_payload: bool,
) -> Result<(), rustpush::PushError> {
    let Some(sender_handle) = message.sender.as_deref() else {
        println!(
            "message.to_raw unavailable (missing sender) for message id {}",
            message.id
        );
        return Ok(());
    };

    let conversation = message
        .conversation
        .as_ref()
        .map(|conv| ConversationData {
            participants: conv.participants.clone(),
            cv_name: conv.cv_name.clone(),
            sender_guid: conv.sender_guid.clone(),
            after_guid: conv.after_guid.clone(),
        })
        .or_else(|| {
            Some(ConversationData {
                participants: vec![sender_handle.to_string()],
                cv_name: None,
                sender_guid: None,
                after_guid: None,
            })
        });

    let Some(conversation) = conversation else {
        println!(
            "message.to_raw unavailable (no conversation data) for message id {}",
            message.id
        );
        return Ok(());
    };

    if !has_payload {
        println!(
            "message.to_raw hex (has_payload=false): <none available> for message id {}",
            message.id
        );
        return Ok(());
    }

    let handles = client.identity.get_handles().await;
    let apns_resource = client.conn.resource.clone();

    let mut temp = MessageInst::new_with_send_delivered(
        conversation,
        sender_handle,
        message.message.clone(),
        Some(message.send_delivered),
    );
    temp.id = message.id.clone();
    temp.target = message.target.clone();

    match temp.to_raw(&handles, apns_resource.as_ref(), false).await {
        Ok(raw_bytes) => {
            println!(
                "message.to_raw hex (has_payload={}): {}",
                has_payload,
                encode_hex(&raw_bytes)
            );
            match plist::Value::from_reader(Cursor::new(&raw_bytes)) {
                Ok(value) => info!("incoming log: {:?}", value),
                Err(err) => info!("incoming log: <unparseable> {err}"),
            }
        }
        Err(err) => {
            let err_string = err.to_string();
            if let Message::Error(error_message) = &message.message {
                let reference_id = parse_reference_message_id(&error_message.for_uuid);
                if let Some(ref_id) = reference_id.as_deref() {
                    info!("error_reference_message_id {}", ref_id);
                } else {
                    info!("error_reference_message_id <missing>");
                }
                if let Some(token_hex) = error_message.token.as_deref() {
                    info!("error_token {}", token_hex);
                    let since_the_epoch: Duration = duration_since_epoch();

                    let mut map = global_message_map.lock().unwrap();

                    let lookup_id = reference_id.as_deref().unwrap_or(&message.id);
                    if let Some(message_timing) = map
                        .get_mut(lookup_id)
                        .and_then(|x| x.iter_mut().find(|t| t.token == token_hex))
                    {
                        message_timing.ts_measured_received = since_the_epoch.as_millis() as u64;
                        let delta_ms = message_timing
                            .ts_measured_received
                            .saturating_sub(message_timing.ts_measured_after_send);
                        let aps_epoch_ns = message_timing.ts_server;
                        let aps_epoch_str = if aps_epoch_ns == 0 {
                            "-".to_string()
                        } else {
                            aps_epoch_ns.to_string()
                        };
                        let aps_epoch_delta_str = if aps_epoch_ns == 0 {
                            "-".to_string()
                        } else {
                            let aps_ms = aps_epoch_ns / 1_000_000;
                            aps_ms
                                .saturating_sub(message_timing.ts_measured_after_send)
                                .to_string()
                        };
                        let aps_epoch_delta_ms = if aps_epoch_ns == 0 {
                            None
                        } else {
                            let aps_ms = aps_epoch_ns / 1_000_000;
                            Some(aps_ms.saturating_sub(message_timing.ts_measured_after_send))
                        };
                        let victim_to_aps_ms = aps_epoch_delta_ms
                            .map(|val| delta_ms.saturating_sub(val))
                            .unwrap_or(0);
                        log_measurement(&format!(
                            "received message response from token {} after {} ms (aps_e={} aps_e_delta_ms={} aps_c={} message_id={}) victim to aps was {} ms",
                            message_timing.token,
                            delta_ms,
                            aps_epoch_str,
                            aps_epoch_delta_str,
                            message.message.get_c(),
                            message.id,
                            victim_to_aps_ms
                            
                        ));
                        //info!("received message response from token {}, {}, {}", message_timing.token, message_timing.ts_measured_received, message_timing.ts_measured_after_send);
                    };
                } else {
                    info!("error without token {}", err_string);
                }
            } else {
                info!("error without token  {}", err_string);
            }
            info!(
                "message.to_raw unavailable (error {}) for message id {}",
                err_string, message.id
            );
        }
    };
    Ok(())
}
