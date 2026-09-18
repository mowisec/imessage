use chrono::{DateTime, Utc};
use log::info;
use rustpush::findmy::MULTIPLEX_SERVICE;
use rustpush::macos::MacOSConfig;
use rustpush::{
    util::encode_hex, APSConnectionResource, APSState, ConversationData, IDSNGMIdentity, IDSUser,
    IMClient, Message, MessageInst, MessageTarget, MessageType, NormalMessage, QueryOptions,
    MADRID_SERVICE,
};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::cmp::Reverse;
use std::env;
use std::fs;
use std::io::{self, Write};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::fs as async_fs;

#[derive(Deserialize)]
struct HwInfo {
    #[serde(rename = "os_config")]
    os_config: MacOSConfig,
    push: APSState,
    identity: IDSNGMIdentity,
}

// Persisted to `<bluebubbles_dir>/group_history.json` so repeated test runs against the same
// group don't require retyping every participant handle each time.
#[derive(Serialize, Deserialize, Clone)]
struct GroupHistoryEntry {
    participants: Vec<String>,
    #[serde(with = "chrono::serde::ts_seconds")]
    last_used: DateTime<Utc>,
}

// Reimplements `common::login` rather than sharing it: this binary's interactive, per-participant
// group workflow (below) doesn't fit the other imessage-sender tools' shared helpers.
async fn login(bluebubble_dir: &str) -> IMClient {
    let hw_info_path = format!("{bluebubble_dir}/hw_info.plist");
    let hw_info_data = async_fs::read(&hw_info_path)
        .await
        .expect("Failed to read hw_info.plist");
    let hw_info: HwInfo =
        plist::from_bytes(&hw_info_data).expect("Failed to deserialize hw_info.plist");

    let config = Arc::new(hw_info.os_config);
    let push_state = hw_info.push;
    let identity = hw_info.identity;

    let users_path = format!("{bluebubble_dir}/id.plist");
    let users_data = async_fs::read(&users_path)
        .await
        .expect("Failed to read id.plist");
    let users: Vec<IDSUser> =
        plist::from_bytes(&users_data).expect("Failed to deserialize id.plist");

    let (connection, error) = APSConnectionResource::new(config.clone(), Some(push_state)).await;
    if let Some(error) = error {
        panic!("Failed to create APS connection: {error}");
    }

    let services = &[&MADRID_SERVICE, &MULTIPLEX_SERVICE];

    IMClient::new(
        connection.clone(),
        users,
        identity,
        services,
        PathBuf::from(format!("{bluebubble_dir}/id_cache.plist")),
        config.clone(),
        Box::new(|_updated_keys| {}),
    )
    .await
}

fn history_path(bluebubble_dir: &str) -> PathBuf {
    PathBuf::from(format!("{bluebubble_dir}/group_history.json"))
}

fn load_history(path: &PathBuf) -> Vec<GroupHistoryEntry> {
    if let Ok(raw) = fs::read(path) {
        if raw.is_empty() {
            return Vec::new();
        }
        serde_json::from_slice(&raw).unwrap_or_default()
    } else {
        Vec::new()
    }
}

fn save_history(path: &PathBuf, mut history: Vec<GroupHistoryEntry>) {
    history.sort_by_key(|entry| Reverse(entry.last_used));
    if let Ok(serialized) = serde_json::to_vec_pretty(&history) {
        if let Some(parent) = path.parent() {
            if let Err(err) = fs::create_dir_all(parent) {
                eprintln!("Failed to create history directory: {err}");
                return;
            }
        }
        if let Err(err) = fs::write(path, serialized) {
            eprintln!("Failed to persist history: {err}");
        }
    }
}

fn prompt(prompt_text: &str) -> io::Result<String> {
    print!("{prompt_text}");
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

// Offers the most-recently-used group first, falling back to a numbered pick list of the rest
// (newest first); returns None if the operator declines both so the caller falls back to
// collecting a fresh participant list.
fn suggest_group(history: &[GroupHistoryEntry]) -> io::Result<Option<Vec<String>>> {
    if history.is_empty() {
        return Ok(None);
    }

    let mut sorted = history.to_vec();
    sorted.sort_by_key(|entry| Reverse(entry.last_used));
    let primary = &sorted[0];
    println!("Most recent group: {}", primary.participants.join(", "));
    if prompt_yes_no("Use this group? (y/n): ")? {
        return Ok(Some(primary.participants.clone()));
    }

    if sorted.len() > 1 && prompt_yes_no("Select another saved group? (y/n): ")? {
        for (idx, entry) in sorted.iter().enumerate() {
            println!("{idx}: {}", entry.participants.join(", "));
        }
        loop {
            let selection = prompt("Enter group number or press Enter to skip: ")?;
            if selection.is_empty() {
                break;
            }
            if let Ok(index) = selection.parse::<usize>() {
                if let Some(entry) = sorted.get(index) {
                    return Ok(Some(entry.participants.clone()));
                }
            }
            println!("Invalid selection; try again.");
        }
    }

    Ok(None)
}

fn collect_group_participants(existing: &[String]) -> io::Result<Vec<String>> {
    println!("Enter group participants one per line. Empty line to finish.");
    println!("Current participants: {}", existing.join(", "));
    let mut participants: Vec<String> = existing.to_vec();
    loop {
        let entry = prompt("Handle (blank to finish): ")?;
        if entry.is_empty() {
            break;
        }
        if participants.contains(&entry) {
            println!("Handle already added.");
            continue;
        }
        participants.push(entry);
    }
    if participants.len() < 2 {
        println!("At least two participants are required for a group chat.");
        return collect_group_participants(existing);
    }
    Ok(participants)
}

fn record_history(path: &PathBuf, history: &mut Vec<GroupHistoryEntry>, participants: &[String]) {
    let mut normalized = participants.to_vec();
    normalized.sort();
    let now = Utc::now();

    if let Some(existing) = history.iter_mut().find(|entry| {
        let mut other = entry.participants.clone();
        other.sort();
        other == normalized
    }) {
        existing.last_used = now;
        existing.participants = participants.to_vec();
    } else {
        history.push(GroupHistoryEntry {
            participants: participants.to_vec(),
            last_used: now,
        });
    }

    save_history(path, history.clone());
}

fn collect_message_text() -> Result<String, String> {
    loop {
        match prompt("Enter message text: ") {
            Ok(text) if !text.trim().is_empty() => return Ok(text),
            Ok(_) => println!("Message cannot be empty."),
            Err(err) => {
                eprintln!("Failed to read message: {err}");
                return Err("input error".to_string());
            }
        }
    }
}

#[tokio::main]
async fn main() {
    if env::var("RUST_LOG").is_err() {
        env::set_var("RUST_LOG", "info");
    }
    pretty_env_logger::try_init().ok();

    let args: Vec<String> = env::args().collect();
    if args.len() != 2 {
        eprintln!("Usage: {} <bluebubbles_dir>", args[0]);
        return;
    }
    let bluebubble_dir = &args[1];

    let client = login(bluebubble_dir).await;

    let handles = client.identity.get_handles().await;
    let Some(sender_handle_ref) = handles.first() else {
        eprintln!("No sender handles available.");
        return;
    };
    let sender_handle = sender_handle_ref.clone();

    let history_file = history_path(bluebubble_dir);
    let mut history = load_history(&history_file);

    let mut group_participants = match suggest_group(&history) {
        Ok(Some(participants)) => participants,
        Ok(None) => vec![],
        Err(err) => {
            eprintln!("Failed to load suggestions: {err}");
            vec![]
        }
    };

    if group_participants.is_empty() {
        match collect_group_participants(&[]) {
            Ok(new_group) => group_participants = new_group,
            Err(err) => {
                eprintln!("Failed to collect group participants: {err}");
                return;
            }
        }
    }

    println!(
        "Using group participants: {}",
        group_participants.join(", ")
    );

    let mut summary_records: Vec<(String, Option<String>)> = Vec::new();

    for participant in &group_participants {
        println!("\n----");
        println!("Processing participant: {participant}");

        // Evict any cached delivery targets for this participant so cache_keys below is forced
        // to do a fresh IDS lookup instead of silently reusing stale (possibly rotated) tokens.
        {
            let mut cache_lock = client.identity.cache.lock().await;
            cache_lock.remove_participant(&sender_handle, participant);
        }

        let query_result = client
            .identity
            .cache_keys(
                MADRID_SERVICE.name,
                &[participant.clone()],
                &sender_handle,
                false,
                &QueryOptions {
                    required_for_message: true,
                    result_expected: true,
                },
            )
            .await;

        if let Err(err) = query_result {
            let error_text = format!("Failed to query keys: {err}");
            eprintln!("{error_text}");
            summary_records.push((participant.clone(), Some(error_text)));
            continue;
        }

        let delivery_handles = {
            let cache_lock = client.identity.cache.lock().await;
            cache_lock.get_participants_targets(
                MADRID_SERVICE.name,
                &sender_handle,
                &[participant.clone()],
            )
        };

        if delivery_handles.is_empty() {
            println!("No delivery targets for {participant}.");
            summary_records.push((participant.clone(), Some("no delivery targets".to_string())));
            continue;
        }

        println!("Found {} delivery target(s).", delivery_handles.len());
        for (idx, handle) in delivery_handles.iter().enumerate() {
            println!(
                "Target {}:\n  push_token: {}\n  session_token: {}\n  expires_in: {}s\n  refresh_after: {}s",
                idx + 1,
                encode_hex(&handle.delivery_data.push_token),
                encode_hex(&handle.delivery_data.session_token),
                handle.delivery_data.session_token_expires_seconds,
                handle.delivery_data.session_token_refresh_seconds,
            );
        }

        match prompt_yes_no(&format!("Send a group message to {participant}? (y/n): ")) {
            Ok(true) => {}
            Ok(false) => {
                println!("Skipping {participant}.");
                summary_records.push((participant.clone(), None));
                continue;
            }
            Err(err) => {
                eprintln!("Input error: {err}");
                summary_records.push((participant.clone(), Some("input error".to_string())));
                continue;
            }
        }

        let message_text = match collect_message_text() {
            Ok(text) => text,
            Err(err_text) => {
                summary_records.push((participant.clone(), Some(err_text)));
                continue;
            }
        };

        let mut message_inst = MessageInst::new(
            ConversationData {
                participants: group_participants.clone(),
                cv_name: None,
                sender_guid: None,
                after_guid: None,
            },
            &sender_handle,
            Message::Message(NormalMessage::new(
                message_text.clone(),
                MessageType::IMessage,
            )),
        );

        message_inst.target = Some(
            delivery_handles
                .iter()
                .map(|delivery| MessageTarget::Token(delivery.delivery_data.push_token.clone()))
                .collect(),
        );

        match client.send(&mut message_inst).await {
            Ok(_) => {
                println!("Message sent to {participant}.");
                summary_records.push((participant.clone(), Some(message_text)));
            }
            Err(err) => {
                let err_text = format!("SEND FAILED: {err}");
                eprintln!("{err_text}");
                summary_records.push((participant.clone(), Some(format!("{err_text}"))));
            }
        }
    }

    if summary_records.is_empty() {
        println!("No participant actions recorded; skipping summary.");
        return;
    }

    match prompt_yes_no("Send summary to your own devices? (y/n): ") {
        Ok(true) => {}
        Ok(false) => {
            println!("Skipping summary.");
            record_history(&history_file, &mut history, &group_participants);
            return;
        }
        Err(err) => {
            eprintln!("Input error: {err}");
            record_history(&history_file, &mut history, &group_participants);
            return;
        }
    }

    let own_devices = match client.identity.get_sms_targets(&sender_handle, false).await {
        Ok(devices) => devices,
        Err(err) => {
            eprintln!("Failed to fetch own devices: {err}");
            record_history(&history_file, &mut history, &group_participants);
            return;
        }
    };

    if own_devices.is_empty() {
        println!("No own devices found.");
        record_history(&history_file, &mut history, &group_participants);
        return;
    }

    let mut summary_map = Map::new();
    for (participant, value) in &summary_records {
        match value {
            Some(text) => {
                summary_map.insert(participant.clone(), Value::String(text.clone()));
            }
            None => {
                summary_map.insert(participant.clone(), Value::String("not sent".to_string()));
            }
        }
    }

    let summary_text = match serde_json::to_string_pretty(&Value::Object(summary_map)) {
        Ok(text) => text,
        Err(err) => {
            eprintln!("Failed to build summary: {err}");
            record_history(&history_file, &mut history, &group_participants);
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
        eprintln!("Failed to prepare own device keys: {err}");
        record_history(&history_file, &mut history, &group_participants);
        return;
    }

    let message_targets: Vec<MessageTarget> = own_devices
        .iter()
        .map(|device| MessageTarget::Token(device.token.clone()))
        .collect();

    let mut summary_inst = MessageInst::new(
        ConversationData {
            participants: vec![sender_handle.clone()],
            cv_name: Some("Group message summary".to_string()),
            sender_guid: None,
            after_guid: None,
        },
        &sender_handle,
        Message::Message(NormalMessage::new(summary_text, MessageType::IMessage)),
    );

    summary_inst.target = Some(message_targets);

    match client.send(&mut summary_inst).await {
        Ok(_) => info!("Summary sent to own devices."),
        Err(err) => eprintln!("Failed to send summary: {err}"),
    }

    record_history(&history_file, &mut history, &group_participants);
}
