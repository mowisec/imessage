use rustpush::findmy::MULTIPLEX_SERVICE;
use rustpush::macos::MacOSConfig;
use rustpush::IDSIdentity;
use rustpush::{
    util::encode_hex, APSConnectionResource, APSState, ConversationData, DeliveryHandle,
    IDSDeliveryData, IDSNGMIdentity, IDSUser, IMClient, Message, MessageInst, MessageTarget,
    MessageType, NormalMessage, PushError, QueryOptions, MADRID_SERVICE,
};
use serde::Deserialize;
use serde_json::{Map, Value};
use std::env;
use std::io::{self, Write};
use std::sync::Arc;
use tokio::fs;

struct DevicePreview {
    participant: String,
    push_token_hex: String,
    session_token_hex: String,
    expires_in: u64,
    refresh_after: u64,
    identity_key_hex: Option<String>,
    push_token_raw: Vec<u8>,
}

#[derive(Deserialize)]
struct HwInfo {
    #[serde(rename = "os_config")]
    os_config: MacOSConfig,
    push: APSState,
    identity: IDSNGMIdentity,
}

fn identity_key_hex(delivery: &IDSDeliveryData) -> Option<String> {
    delivery
        .client_data
        .public_message_identity_key
        .encode()
        .ok()
        .map(|bytes| encode_hex(bytes.as_slice()))
}

async fn login(bluebubble_dir: &str) -> IMClient {
    let hw_info_data = fs::read(format!("{}/hw_info.plist", bluebubble_dir))
        .await
        .expect("Failed to read hw_info.plist");
    let hw_info: HwInfo =
        plist::from_bytes(&hw_info_data).expect("Failed to deserialize hw_info.plist");

    let config = Arc::new(hw_info.os_config);
    let push_state = hw_info.push;
    let identity = hw_info.identity;

    let users_data = fs::read(format!("{}/id.plist", bluebubble_dir))
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

fn prompt(prompt_text: &str) -> io::Result<String> {
    print!("{}", prompt_text);
    io::stdout().flush()?;
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    Ok(input.trim().to_string())
}

fn prompt_yes_no(prompt_text: &str) -> io::Result<bool> {
    loop {
        let response = prompt(prompt_text)?;
        match response.to_lowercase().as_str() {
            "y" | "yes" => return Ok(true),
            "n" | "no" => return Ok(false),
            _ => println!("Please answer y or n."),
        }
    }
}

#[tokio::main]
async fn main() {
    if let Err(_) = std::env::var("RUST_LOG") {
        std::env::set_var("RUST_LOG", "debug");
    }
    pretty_env_logger::try_init().unwrap();

    let args: Vec<String> = env::args().collect();
    if args.len() != 3 {
        eprintln!("Usage: {} <bluebubbles_dir> <target_handle>", args[0]);
        return;
    }
    let bluebubbles_dir = &args[1];
    let target_handle = &args[2];

    // Reimplements `common::login` rather than sharing it: this binary drives an interactive,
    // per-device workflow (query every device registered to a handle, then let the operator
    // pick which individual device to message) that the other imessage-sender tools don't need.
    let client = login(bluebubbles_dir).await;

    let handles = client.identity.get_handles().await;
    let Some(sender_handle_ref) = handles.first() else {
        eprintln!("No sender handles available.");
        return;
    };
    let sender_handle = sender_handle_ref.clone();

    println!("Using sender handle: {}", sender_handle);
    println!("Querying devices for target: {}", target_handle);

    // (summary key -> what was sent/skipped for that device), used to build the own-devices
    // summary message at the end of the run.
    let mut device_action_log: Vec<(String, Option<String>)> = Vec::new();

    let delivery_handles = match client
        .identity
        .resource
        .query_participant_targets(
            MADRID_SERVICE.name,
            &[target_handle.to_string()],
            &sender_handle,
            &QueryOptions {
                required_for_message: true,
                result_expected: true,
            },
        )
        .await
    {
        Ok(handles) => handles,
        Err(err) => {
            eprintln!("Failed to query device keys: {}", err);
            return;
        }
    };

    if delivery_handles.is_empty() {
        println!("No devices found for target {}.", target_handle);
    } else {
        let device_previews: Vec<DevicePreview> = delivery_handles
            .iter()
            .map(|delivery_handle| DevicePreview {
                participant: delivery_handle.participant.clone(),
                push_token_hex: encode_hex(&delivery_handle.delivery_data.push_token),
                session_token_hex: encode_hex(&delivery_handle.delivery_data.session_token),
                expires_in: delivery_handle.delivery_data.session_token_expires_seconds,
                refresh_after: delivery_handle.delivery_data.session_token_refresh_seconds,
                identity_key_hex: identity_key_hex(&delivery_handle.delivery_data),
                push_token_raw: delivery_handle.delivery_data.push_token.clone(),
            })
            .collect();

        println!("Found {} device(s).", device_previews.len());
        println!("----------------------------------------");

        for (idx, device) in device_previews.iter().enumerate() {
            // "clientN" keys let the own-device summary correlate outcomes back to the Nth
            // device listed above without re-exposing push tokens in the summary message.
            let entry_key = format!("client{}", idx + 1);
            println!(
                "Device {}:\n  push_token: {}\n  session_token: {}\n  expires_in: {}s\n  refresh_after: {}s",
                idx + 1,
                device.push_token_hex,
                device.session_token_hex,
                device.expires_in,
                device.refresh_after,
            );

            match prompt_yes_no("Send message to this device? (y/n): ") {
                Ok(true) => {
                    let message_text = loop {
                        match prompt("Enter message text: ") {
                            Ok(text) if !text.trim().is_empty() => break text,
                            Ok(_) => println!("Message cannot be empty."),
                            Err(err) => {
                                eprintln!("Failed to read message: {}", err);
                                return;
                            }
                        }
                    };

                    let mut recorded_value = Some(message_text.clone());

                    let normal_message =
                        NormalMessage::new(message_text.clone(), MessageType::IMessage);
                    let mut message_inst = MessageInst::new(
                        ConversationData {
                            participants: vec![target_handle.to_string()],
                            cv_name: None,
                            sender_guid: None,
                            after_guid: None,
                        },
                        &sender_handle,
                        Message::Message(normal_message),
                    );

                    match refresh_delivery_handle(
                        &client,
                        &sender_handle,
                        target_handle,
                        device.identity_key_hex.as_deref(),
                        &device.push_token_raw,
                    )
                    .await
                    {
                        Ok(refreshed_handle) => {
                            message_inst.target = Some(vec![MessageTarget::Token(
                                refreshed_handle.delivery_data.push_token.clone(),
                            )]);

                            match client.send(&mut message_inst).await {
                                Ok(_) => println!("Message sent successfully."),
                                Err(err) => {
                                    let err_text = err.to_string();
                                    eprintln!("Error sending message: {}", err_text);
                                    recorded_value = Some(format!(
                                        "SEND FAILED: {} | message: {}",
                                        err_text, message_text
                                    ));
                                }
                            }
                        }
                        Err(err) => {
                            let err_text = err.to_string();
                            eprintln!("Failed to refresh device info: {}", err_text);
                            recorded_value = Some(format!(
                                "REFRESH FAILED: {} | message: {}",
                                err_text, message_text
                            ));
                        }
                    }

                    device_action_log.push((entry_key, recorded_value));
                }
                Ok(false) => {
                    println!("Skipping device {}.", idx + 1);
                    device_action_log.push((entry_key, None));
                }
                Err(err) => {
                    eprintln!("Input error: {}", err);
                    return;
                }
            }

            println!("----------------------------------------");
        }
    }

    if device_action_log.is_empty() {
        println!("No remote device actions recorded; skipping own-device summary.");
        return;
    }

    match prompt_yes_no("Send a summary message to your own devices? (y/n): ") {
        Ok(true) => {}
        Ok(false) => {
            println!("Skipping messages to own devices.");
            return;
        }
        Err(err) => {
            eprintln!("Input error: {}", err);
            return;
        }
    }

    let own_devices = match client.identity.get_sms_targets(&sender_handle, false).await {
        Ok(devices) => devices,
        Err(err) => {
            eprintln!("Failed to fetch own devices: {}", err);
            return;
        }
    };

    if own_devices.is_empty() {
        println!("No own devices found to message.");
        return;
    }

    let mut summary_map = Map::new();
    for (key, value) in &device_action_log {
        match value {
            Some(text) => {
                summary_map.insert(key.clone(), Value::String(text.clone()));
            }
            None => {
                summary_map.insert(key.clone(), Value::String("not send".to_string()));
            }
        }
    }

    let summary_text = match serde_json::to_string_pretty(&Value::Object(summary_map)) {
        Ok(text) => text,
        Err(err) => {
            eprintln!("Failed to build summary message: {}", err);
            return;
        }
    };

    if let Err(err) = client
        .identity
        .cache_keys(
            MADRID_SERVICE.name,
            &[sender_handle.clone()],
            &sender_handle,
            false,
            &QueryOptions {
                required_for_message: true,
                result_expected: true,
            },
        )
        .await
    {
        eprintln!("Failed to prepare keys for own devices: {}", err);
        return;
    }

    let message_targets: Vec<MessageTarget> = own_devices
        .iter()
        .map(|device| MessageTarget::Token(device.token.clone()))
        .collect();

    if message_targets.is_empty() {
        println!("No valid targets for own devices.");
        return;
    }

    let mut message_inst = MessageInst::new(
        ConversationData {
            participants: vec![sender_handle.clone()],
            cv_name: Some(format!("Summary for {}", target_handle)),
            sender_guid: None,
            after_guid: None,
        },
        &sender_handle,
        Message::Message(NormalMessage::new(summary_text, MessageType::IMessage)),
    );

    message_inst.target = Some(message_targets);

    match client.send(&mut message_inst).await {
        Ok(_) => println!("Summary message sent to own devices."),
        Err(err) => eprintln!("Error sending summary message: {}", err),
    }
}

// Push tokens/session tokens can rotate between the initial device query and when the operator
// actually confirms sending, so this re-queries IDS and re-identifies the same physical device
// (by its identity key, falling back to the previously seen push token) rather than trusting the
// stale DeliveryHandle captured earlier.
async fn refresh_delivery_handle(
    client: &IMClient,
    sender_handle: &str,
    target_handle: &str,
    identity_key_hint: Option<&str>,
    fallback_push_token: &[u8],
) -> Result<DeliveryHandle, PushError> {
    let mut refreshed = client
        .identity
        .targets_for_handles(
            MADRID_SERVICE.name,
            &[target_handle.to_string()],
            sender_handle,
        )
        .await?;

    if let Some(identity_hex) = identity_key_hint {
        if let Some(position) = refreshed.iter().position(|handle| {
            identity_key_hex(&handle.delivery_data)
                .as_deref()
                .map(|candidate| candidate == identity_hex)
                .unwrap_or(false)
        }) {
            return Ok(refreshed.remove(position));
        }
    }

    if let Some(position) = refreshed
        .iter()
        .position(|handle| handle.delivery_data.push_token.as_slice() == fallback_push_token)
    {
        return Ok(refreshed.remove(position));
    }

    refreshed
        .into_iter()
        .next()
        .ok_or(PushError::NoValidTargets)
}
