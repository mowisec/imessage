#[path = "common.rs"]
mod common;

use common::{
    listen_for_messages, listen_for_messages_for, login, send_ipc_request, test_message_types,
    IpcRequest, DEFAULT_IPC_SOCKET,
};
use rustpush::util::decode_hex;
use std::env;
use std::path::Path;
use tokio::time::Duration;

// Drives common::test_message_types, which sends one instance of every rustpush::Message
// variant (mostly with placeholder/empty fields) to the target. Different iMessage client
// versions/OSes accept, reject, or otherwise react differently to each message type, which is
// what this suite is used to probe; --message-index isolates a single type for follow-up.
fn print_usage(exe: &str) {
    eprintln!(
        "Usage: {exe} <bluebubbles_dir> <target_handle> [--stop] [--listen] [--ipc-socket <path>] [--no-ipc] [--push-token <hex>] [--message-index <n>]

Send a suite of message types to the target handle. Defaults to sending typing=true
for the typing indicator entry. Use --stop to send typing=false instead. Use
--ipc-socket to forward the request to a running listener process to avoid APS
connection conflicts (defaults to {DEFAULT_IPC_SOCKET}); use --no-ipc to connect
directly. Use --push-token <hex> to target only a specific push token. Use
--message-index <n> (1-based) to send only a single message from the vector."
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

    let mut typing = true;
    let mut keep_listening = false;
    let mut ipc_socket: Option<String> = Some(DEFAULT_IPC_SOCKET.to_string());
    let mut push_token_hex: Option<String> = None;
    let mut message_index: Option<usize> = None;

    let mut idx = 3;
    while idx < args.len() {
        match args[idx].as_str() {
            "--stop" => {
                typing = false;
            }
            "--listen" => {
                keep_listening = true;
            }
            "--ipc-socket" => {
                if idx + 1 >= args.len() {
                    eprintln!("--ipc-socket requires a path");
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
                    eprintln!("--push-token requires a hex string");
                    print_usage(&args[0]);
                    return;
                }
                push_token_hex = Some(args[idx + 1].clone());
                idx += 1;
            }
            "--message-index" => {
                if idx + 1 >= args.len() {
                    eprintln!("--message-index requires a number");
                    print_usage(&args[0]);
                    return;
                }
                let parsed = match args[idx + 1].parse::<usize>() {
                    Ok(value) => value,
                    Err(_) => {
                        eprintln!("--message-index must be a number");
                        print_usage(&args[0]);
                        return;
                    }
                };
                message_index = Some(parsed);
                idx += 1;
            }
            "--help" | "-h" => {
                print_usage(&args[0]);
                return;
            }
            flag => {
                eprintln!("Unknown flag: {flag}");
                print_usage(&args[0]);
                return;
            }
        }
        idx += 1;
    }

    if keep_listening && ipc_socket.is_some() {
        eprintln!("--listen cannot be combined with --ipc-socket");
        return;
    }

    if let Some(socket) = ipc_socket {
        match send_ipc_request(
            Path::new(&socket),
            &IpcRequest::TestMessageTypes {
                target: target_handle.to_string(),
                typing,
                push_token: push_token_hex.clone(),
                message_index,
            },
        )
        .await
        {
            Ok(resp) if resp.ok => {
                println!("Sent test message suite to {} via IPC", target_handle);
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
    let push_token = match push_token_hex {
        Some(hex) => match decode_hex(&hex) {
            Ok(bytes) => Some(bytes),
            Err(err) => {
                eprintln!("Invalid --push-token hex: {err}");
                return;
            }
        },
        None => None,
    };

    listen_for_messages_for(&client, Duration::from_secs(5)).await;

    if let Err(err) =
        test_message_types(&client, target_handle, typing, None, push_token, message_index).await
    {
        eprintln!("Failed to send test message suite: {err}");
        return;
    }

    if keep_listening {
        listen_for_messages(&client).await;
    } else {
        listen_for_messages_for(&client, Duration::from_secs(5)).await;
    }
}
