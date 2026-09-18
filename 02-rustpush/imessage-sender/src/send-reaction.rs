#[path = "common.rs"]
mod common;

use common::{
    login, parse_participants, send_ipc_request, send_reaction_message, IpcRequest,
    DEFAULT_IPC_SOCKET,
};
use rustpush::imessage::messages::Reaction;
use std::env;
use std::fs;
use std::path::Path;

fn print_usage(exe: &str) {
    eprintln!(
        "Usage: {exe} <bluebubbles_dir> <target_handle> <xml text> [--amt <number>] [--no-amt] [--read-from-file <path>] [--no-ipc]

Send a reaction with custom XML text (`t` value) to the target handle using default settings. Use --no-ipc to send directly instead of via IPC."
    );
}

// `amt` is Apple's wire-level "association message type" code: 2000-2005 add a tapback
// (heart/like/dislike/laugh/emphasize/question, in that order) and 3000-3005 remove the same
// one. This lets --amt target a specific tapback numerically without spelling it out, and lets
// the reaction/enable pair be reconstructed from a captured amt value.
fn reaction_from_amt(amt: u64) -> Option<(Reaction, bool)> {
    let enable = amt < 3000;
    let base = if enable { amt.checked_sub(2000)? } else { amt.checked_sub(3000)? };
    let reaction = match base {
        0 => Reaction::Heart,
        1 => Reaction::Like,
        2 => Reaction::Dislike,
        3 => Reaction::Laugh,
        4 => Reaction::Emphasize,
        5 => Reaction::Question,
        _ => return None,
    };
    Some((reaction, enable))
}

fn reaction_cmd(reaction: &Reaction, enable: bool) -> u64 {
    let idx = match reaction {
        Reaction::Heart => 0,
        Reaction::Like => 1,
        Reaction::Dislike => 2,
        Reaction::Laugh => 3,
        Reaction::Emphasize => 4,
        Reaction::Question => 5,
        Reaction::Emoji(_) => 6,
        Reaction::Sticker { .. } => 7,
    };
    if enable {
        2000 + idx
    } else {
        3000 + idx
    }
}

fn reaction_label(reaction: &Reaction) -> &'static str {
    match reaction {
        Reaction::Heart => "love",
        Reaction::Like => "like",
        Reaction::Dislike => "dislike",
        Reaction::Laugh => "laugh",
        Reaction::Emphasize => "emphasize",
        Reaction::Question => "question",
        Reaction::Emoji(_) => "emoji",
        Reaction::Sticker { .. } => "sticker",
    }
}

fn reaction_emoji(reaction: &Reaction) -> Option<String> {
    match reaction {
        Reaction::Emoji(e) => Some(e.clone()),
        _ => None,
    }
}

#[tokio::main]
async fn main() {
    if std::env::var("RUST_LOG").is_err() {
        std::env::set_var("RUST_LOG", "info");
    }
    pretty_env_logger::try_init().unwrap();

    let args: Vec<String> = env::args().collect();
    if args.len() < 4 {
        print_usage(&args[0]);
        return;
    }

    let bluebubbles_dir = &args[1];
    let target_handle = &args[2];
    let mut xml_text = args[3].clone();
    let mut ipc_socket: Option<String> = Some(DEFAULT_IPC_SOCKET.to_string());
    let mut custom_amt: Option<u64> = None;
    let mut omit_amt = false;
    let mut xml_path: Option<String> = None;

    let mut idx = 4;
    while idx < args.len() {
        match args[idx].as_str() {
            "--no-ipc" => {
                ipc_socket = None;
            }
            "--no-amt" => {
                omit_amt = true;
            }
            "--read-from-file" => {
                if idx + 1 >= args.len() {
                    eprintln!("--read-from-file requires a path");
                    print_usage(&args[0]);
                    return;
                }
                xml_path = Some(args[idx + 1].clone());
                idx += 1;
            }
            "--amt" => {
                if idx + 1 >= args.len() {
                    eprintln!("--amt requires a value");
                    print_usage(&args[0]);
                    return;
                }
                match args[idx + 1].parse::<u64>() {
                    Ok(v) => custom_amt = Some(v),
                    Err(e) => {
                        eprintln!("Invalid amt value: {e}");
                        return;
                    }
                }
                idx += 1;
            }
            other => {
                eprintln!("Unknown option: {other}");
                print_usage(&args[0]);
                return;
            }
        }
        idx += 1;
    }

    if let Some(path) = xml_path {
        match fs::read_to_string(&path) {
            Ok(contents) => xml_text = contents,
            Err(err) => {
                eprintln!("Failed to read XML from {path}: {err}");
                return;
            }
        }
    }

    if omit_amt && custom_amt.is_some() {
        eprintln!("--amt and --no-amt cannot be used together");
        return;
    }

    let (reaction, enable_reaction, amt_to_send) = if omit_amt {
        (Reaction::Like, true, None)
    } else {
        let (reaction, enable) = custom_amt
            .and_then(reaction_from_amt)
            .unwrap_or((Reaction::Like, true));
        let amt = Some(custom_amt.unwrap_or_else(|| reaction_cmd(&reaction, enable)));
        (reaction, enable, amt)
    };
    let part_index: u64 = 0;
    // This tool sends a standalone reaction rather than reacting to a real prior message, so
    // there's no target message GUID/original text to reference — both are left blank and the
    // <xml text> argument is carried instead as the reaction's custom `t` (text) value.
    let target_guid = "";
    let original_text = String::new();

    if let Some(socket) = ipc_socket {
        match send_ipc_request(
            Path::new(&socket),
            &IpcRequest::SendReaction {
                target: target_handle.to_string(),
                guid: target_guid.to_string(),
                reaction: reaction_label(&reaction).to_string(),
                enable: enable_reaction,
                text: None,
                t_value: Some(xml_text.clone()),
                amt: amt_to_send,
                part_index,
                extra_participants: None,
                emoji: reaction_emoji(&reaction),
                push_token: None,
            },
        )
        .await
        {
            Ok(resp) if resp.ok => {
                if let Some(amt) = amt_to_send {
                    println!(
                        "Sent reaction via IPC to {} with xml text: {} (amt: {})",
                        target_handle, xml_text, amt
                    );
                } else {
                    println!(
                        "Sent reaction via IPC to {} with xml text: {} (amt omitted)",
                        target_handle, xml_text
                    );
                }
            }
            Ok(resp) => {
                eprintln!(
                    "IPC send failed{}",
                    resp.error
                        .as_deref()
                        .map(|e| format!(": {e}"))
                        .unwrap_or_default()
                );
            }
            Err(err) => {
                eprintln!("Failed to send IPC request to {socket}: {err}");
            }
        }
        return;
    }

    let client = login(bluebubbles_dir).await;

    match send_reaction_message(
        &client,
        target_handle,
        target_guid,
        reaction,
        enable_reaction,
        original_text,
        Some(xml_text.clone()),
        amt_to_send,
        part_index,
        parse_participants(target_handle, None),
        None,
        None,
    )
    .await
    {
        Ok(_) => {
            if let Some(amt) = amt_to_send {
                println!(
                    "Sent reaction to {} with xml text: {} (amt: {})",
                    target_handle, xml_text, amt
                );
            } else {
                println!(
                    "Sent reaction to {} with xml text: {} (amt omitted)",
                    target_handle, xml_text
                );
            }
        }
        Err(err) => {
            eprintln!("Failed to send reaction: {err}");
        }
    }
}
