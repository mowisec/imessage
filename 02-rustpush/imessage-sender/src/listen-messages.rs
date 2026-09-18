#[path = "common.rs"]
mod common;

use common::{
    listen_for_messages_with_logger, login, spawn_ipc_server, MessageLogger, DEFAULT_IPC_SOCKET,
};
use std::env;
use std::path::{Path, PathBuf};
use std::sync::Arc;

fn print_usage(exe: &str) {
    eprintln!(
        "Usage: {exe} <bluebubbles_dir> [--ipc-socket <path>] [--no-ipc] [--db <path>] [--download-dir <path>]

Defaults to starting an IPC server at {DEFAULT_IPC_SOCKET}; use --ipc-socket to override
or --no-ipc to disable. Default DB path is ./imessage.sqlite; override with --db.
Attachments are downloaded automatically; default download directory is /tmp."
    );
}

#[tokio::main]
async fn main() {
    if std::env::var("RUST_LOG").is_err() {
        std::env::set_var("RUST_LOG", "info");
    }
    pretty_env_logger::try_init().unwrap();

    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        print_usage(&args[0]);
        return;
    }

    let bluebubbles_dir = &args[1];
    let mut ipc_socket: Option<String> = Some(DEFAULT_IPC_SOCKET.to_string());
    let mut db_path: PathBuf = PathBuf::from("./imessage.sqlite");
    let mut download_dir: PathBuf = PathBuf::from("/tmp");

    let mut idx = 2;
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
            "--db" => {
                if idx + 1 >= args.len() {
                    eprintln!("--db requires a path");
                    print_usage(&args[0]);
                    return;
                }
                db_path = PathBuf::from(args[idx + 1].clone());
                idx += 1;
            }
            "--download-dir" => {
                if idx + 1 >= args.len() {
                    eprintln!("--download-dir requires a path");
                    print_usage(&args[0]);
                    return;
                }
                download_dir = PathBuf::from(args[idx + 1].clone());
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

    // This is the one long-lived process that actually owns the APS connection for the
    // identity: Apple's push service only allows a single live connection per registered
    // device/identity, so every other imessage-sender binary talks to this process over the
    // IPC socket instead of logging in and connecting a second time.
    let client = Arc::new(login(bluebubbles_dir).await);

    let logger = match MessageLogger::new(db_path.clone()) {
        Ok(logger) => Some(Arc::new(logger)),
        Err(err) => {
            eprintln!("Failed to initialize SQLite logging: {err}");
            None
        }
    };

    if let Some(socket) = ipc_socket {
        match spawn_ipc_server(client.clone(), Path::new(&socket), logger.clone()).await {
            Ok(_) => println!("IPC server listening on {socket}"),
            Err(err) => eprintln!("Failed to start IPC server on {socket}: {err}"),
        }
    }

    // Never returns; this is what keeps the process (and the APS connection) alive.
    listen_for_messages_with_logger(
        client.as_ref(),
        logger.as_deref(),
        Some(download_dir),
    )
    .await;
}
