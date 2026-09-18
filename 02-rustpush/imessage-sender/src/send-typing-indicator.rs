#[path = "common.rs"]
mod common;

use common::{
    listen_for_messages_for, login, send_ipc_request, send_typing_indicator, IpcRequest,
    DEFAULT_IPC_SOCKET,
};
use chrono::Utc;
use log::error;
use rustpush::util::decode_hex;
use std::env;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::Path;
use tokio::time::Duration;

#[derive(Copy, Clone)]
enum TypingState {
    Start,
    Stop,
}

impl TypingState {
    fn as_flag(self) -> bool {
        matches!(self, TypingState::Start)
    }
}

fn log_error(msg: &str) {
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

fn print_usage(exe: &str) {
    eprintln!(
        "Usage: {exe} <bluebubbles_dir> <target_handle> [--stop] [--listen] [--ipc-socket <path>] [--no-ipc] [--push-token <hex>] [--repeat <count>] [--delay <ms>] [--no-force-delivery-receipt]

Send a typing indicator to the target. By default this sends the \"typing\" state.
Provide --stop to send a stop-typing indicator instead. Add --listen to keep
listening for incoming events after sending. Use --ipc-socket to forward the send
request to a running listener process to avoid APS connection conflicts. Defaults to
{DEFAULT_IPC_SOCKET}; use --no-ipc to connect directly (keeps listening for 15 seconds
after sending). Use --push-token <hex> to target only a specific push token. Use --repeat <count>
to send multiple typing indicators
in a row; combine with --delay <ms> to pause between sends."
    );
}

#[tokio::main]
async fn main() {
    if std::env::var("RUST_LOG").is_err() {
        std::env::set_var("RUST_LOG", "info");
    }
    pretty_env_logger::try_init().unwrap();

    let args: Vec<String> = env::args().collect();
    if args.len() < 3 {
        print_usage(&args[0]);
        return;
    }

    let bluebubbles_dir = &args[1];
    let target_handle = &args[2];

    let mut state = TypingState::Start;
    let mut keep_listening = false;
    let mut ipc_socket: Option<String> = Some(DEFAULT_IPC_SOCKET.to_string());
    let mut push_token_hex: Option<String> = None;
    let mut repeat: u64 = 1;
    let mut delay_ms: u64 = 0;
    let mut force_delivery_receipt = true;

    let mut idx = 3;
    while idx < args.len() {
        match args[idx].as_str() {
            "--stop" => {
                state = TypingState::Stop;
            }
            "--listen" => {
                keep_listening = true;
            }
            "--ipc-socket" => {
                if idx + 1 >= args.len() {
                    log_error("--ipc-socket requires a path");
                    print_usage(&args[0]);
                    return;
                }
                ipc_socket = Some(args[idx + 1].clone());
                idx += 1;
            }
            "--no-ipc" => {
                ipc_socket = None;
            }
            "--push-token" => {
                if idx + 1 >= args.len() {
                    log_error("--push-token requires a hex string");
                    print_usage(&args[0]);
                    return;
                }
                push_token_hex = Some(args[idx + 1].clone());
                idx += 1;
            }
            "--repeat" => {
                if idx + 1 >= args.len() {
                    log_error("--repeat requires a positive integer");
                    print_usage(&args[0]);
                    return;
                }
                repeat = match args[idx + 1].parse::<u64>() {
                    Ok(value) if value > 0 => value,
                    _ => {
                        log_error("--repeat must be a positive integer");
                        return;
                    }
                };
                idx += 1;
            }
            "--delay" => {
                if idx + 1 >= args.len() {
                    log_error("--delay requires a number of milliseconds");
                    print_usage(&args[0]);
                    return;
                }
                delay_ms = match args[idx + 1].parse::<u64>() {
                    Ok(value) => value,
                    Err(err) => {
                        log_error(&format!("Invalid --delay value: {err}"));
                        return;
                    }
                };
                idx += 1;
            }
            // Typing indicators normally don't request a delivery receipt. Forcing one lets us
            // observe when/whether the recipient device actually received the indicator, which
            // is what the timing measurements in listen_for_messages_for rely on.
            "--no-force-delivery-receipt" => {
                force_delivery_receipt = false;
            }
            "--help" | "-h" => {
                print_usage(&args[0]);
                return;
            }
            flag => {
                log_error(&format!("Unknown flag: {flag}"));
                print_usage(&args[0]);
                return;
            }
        }
        idx += 1;
    }

    if keep_listening && ipc_socket.is_some() {
        log_error("--listen cannot be combined with --ipc-socket");
        return;
    }

    if let Some(socket) = ipc_socket {
        for attempt in 0..repeat {
            match send_ipc_request(
                Path::new(&socket),
                &IpcRequest::SendTyping {
                    target: target_handle.to_string(),
                    typing: state.as_flag(),
                    push_token: push_token_hex.clone(),
                    send_delivered: Some(force_delivery_receipt),
                },
            )
            .await
            {
                Ok(resp) if resp.ok => {
                    println!(
                        "Sent {} indicator to {} via IPC{}",
                        if state.as_flag() {
                            "typing"
                        } else {
                            "stop typing"
                        },
                        target_handle,
                        if repeat > 1 {
                            format!(" ({} of {})", attempt + 1, repeat)
                        } else {
                            String::new()
                        }
                    );
                }
                Ok(resp) => {
                    log_error(
                        &format!(
                            "IPC send failed{}",
                            resp.error
                                .as_deref()
                                .map(|e| format!(": {e}"))
                                .unwrap_or_default()
                        ),
                    );
                    return;
                }
                Err(err) => {
                    log_error(&format!("Failed to send IPC request to {socket}: {err}"));
                    return;
                }
            }

            if attempt + 1 < repeat && delay_ms > 0 {
                tokio::time::sleep(Duration::from_millis(delay_ms)).await;
            }
        }
        return;
    }

    let client = login(bluebubbles_dir).await;

    let push_token = match push_token_hex {
        Some(hex) => match decode_hex(&hex) {
            Ok(bytes) => Some(bytes),
            Err(err) => {
                log_error(&format!("Invalid --push-token hex: {err}"));
                return;
            }
        },
        None => None,
    };

    // Listen for a few seconds before sending so any delivery/read receipts already in flight
    // for this handle are drained and don't get misattributed to the indicator(s) sent below.
    listen_for_messages_for(&client, Duration::from_secs(5)).await;

    for attempt in 0..repeat {
        match send_typing_indicator(
            &client,
            target_handle,
            state.as_flag(),
            push_token.clone(),
            force_delivery_receipt,
        )
            .await
        {
            Ok(_) => {
                println!(
                    "Sent {} indicator to {}{}",
                    if state.as_flag() {
                        "typing"
                    } else {
                        "stop typing"
                    },
                    target_handle,
                    if repeat > 1 {
                        format!(" ({} of {})", attempt + 1, repeat)
                    } else {
                        String::new()
                    }
                );
            }
            Err(err) => {
                log_error(&format!("Failed to send indicator: {err}"));
                return;
            }
        }

        // Listen (rather than plain sleep) during the inter-send delay so the receipt for this
        // indicator is captured before the next one is sent.
        if attempt + 1 < repeat && delay_ms > 0 {
            listen_for_messages_for(&client, Duration::from_millis(delay_ms)).await;
        }
    }

    // Grace period to catch the receipt for the final indicator before exiting.
    listen_for_messages_for(&client, Duration::from_secs(5)).await;
}
