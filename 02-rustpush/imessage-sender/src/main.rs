#[path = "common.rs"]
mod common;

use common::{
    login, send_ipc_request, send_text_message_with_logging_cached, IpcRequest,
    DEFAULT_IPC_SOCKET,
};
use rustpush::util::decode_hex;
use std::env;
use std::path::Path;

fn print_usage(exe: &str) {
    eprintln!(
        "Usage: {exe} <recipient> <message> <bluebubbles_dir> [--ipc-socket <path>] [--no-ipc] [--push-token <hex>] [--xml <body>] [--repeat-count <n>] [--repeat-delay-ms <ms>]

Defaults to using IPC at {DEFAULT_IPC_SOCKET}; use --ipc-socket to override or --no-ipc to send
directly. Use --push-token <hex> to target a specific device. Use --xml to set the x (XML) body.
Use --repeat-count to send multiple times with an optional --repeat-delay-ms pause."
    );
}

#[tokio::main]
async fn main() {
    if std::env::var("RUST_LOG").is_err() {
        std::env::set_var("RUST_LOG", "debug");
    }
    pretty_env_logger::try_init().unwrap();

    let args: Vec<String> = env::args().collect();
    if args.len() < 4 {
        print_usage(&args[0]);
        return;
    }

    let recipient = &args[1];
    let message = &args[2];
    let bluebubbles_dir = &args[3];

    let mut ipc_socket: Option<String> = Some(DEFAULT_IPC_SOCKET.to_string());
    let mut push_token_hex: Option<String> = None;
    let mut xml_body: Option<String> = None;

    let mut repeat_count: usize = 1;
    let mut repeat_delay_ms: u64 = 0;

    let mut idx = 4;
    while idx < args.len() {
        match args[idx].as_str() {
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
            "--xml" => {
                if idx + 1 >= args.len() {
                    eprintln!("--xml requires a string");
                    print_usage(&args[0]);
                    return;
                }
                xml_body = Some(args[idx + 1].clone());
                idx += 1;
            }
            "--repeat-count" => {
                if idx + 1 >= args.len() {
                    eprintln!("--repeat-count requires a number");
                    print_usage(&args[0]);
                    return;
                }
                repeat_count = match args[idx + 1].parse::<usize>() {
                    Ok(value) if value > 0 => value,
                    _ => {
                        eprintln!("--repeat-count must be a positive number");
                        return;
                    }
                };
                idx += 1;
            }
            "--repeat-delay-ms" => {
                if idx + 1 >= args.len() {
                    eprintln!("--repeat-delay-ms requires a number");
                    print_usage(&args[0]);
                    return;
                }
                repeat_delay_ms = match args[idx + 1].parse::<u64>() {
                    Ok(value) => value,
                    Err(_) => {
                        eprintln!("--repeat-delay-ms must be a number");
                        return;
                    }
                };
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

    // Forward the send to an already-running `imessage-listen` process rather than opening a
    // second APS connection for the same identity (Apple only allows one live connection per
    // device/identity at a time).
    if let Some(socket) = ipc_socket {
        for attempt in 0..repeat_count {
            match send_ipc_request(
                Path::new(&socket),
                &IpcRequest::SendMessage {
                    target: recipient.to_string(),
                    text: message.to_string(),
                    xml: xml_body.clone(),
                    push_token: push_token_hex.clone(),
                },
            )
            .await
            {
                Ok(resp) if resp.ok => {
                    println!("Message sent to {recipient} via IPC");
                }
                Ok(resp) => {
                    eprintln!(
                        "IPC send failed{}",
                        resp.error
                            .as_deref()
                            .map(|err| format!(": {err}"))
                            .unwrap_or_default()
                    );
                }
                Err(err) => {
                    eprintln!("Failed to send IPC request to {socket}: {err}");
                }
            }

            if attempt + 1 < repeat_count && repeat_delay_ms > 0 {
                tokio::time::sleep(std::time::Duration::from_millis(repeat_delay_ms)).await;
            }
        }
        return;
    }

    // --push-token pins delivery to one specific device rather than every device registered to
    // the recipient handle; see common::send_text_message_with_logging_cached for how this
    // narrows the conversation participants down to a single explicit MessageTarget::Token.
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

    let client = login(bluebubbles_dir).await;

    // --no-ipc path: open our own APS connection and send directly. Uses the "cached targets"
    // variant so a --repeat-count run doesn't re-query IDS for delivery targets on every send.
    for attempt in 0..repeat_count {
        if let Err(err) = send_text_message_with_logging_cached(
            &client,
            recipient,
            message,
            xml_body.clone(),
            None,
            push_token.clone(),
        )
        .await
        {
            eprintln!("Error sending message: {err}");
        } else {
            println!("Message sent successfully!");
        }

        if attempt + 1 < repeat_count && repeat_delay_ms > 0 {
            tokio::time::sleep(std::time::Duration::from_millis(repeat_delay_ms)).await;
        }
    }
}
