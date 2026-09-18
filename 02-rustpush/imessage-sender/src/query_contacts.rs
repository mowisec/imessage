use log::{debug, info, warn};
use rand::{thread_rng, Rng};
use rusqlite::Connection;
use rustpush::{
    imessage::aps_client::{IMClient, MADRID_SERVICE},
    imessage::query_client::QueryClient,
    macos::MacOSConfig,
    APSConnectionResource, APSState, IDSNGMIdentity, IDSUser, QueryOptions,
};
use serde::Deserialize;
use std::collections::HashSet;
use std::env;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tokio::fs;
use tokio::time::sleep;
use uuid::Uuid;

// A handle appended to every chunk sent to IDS, e.g. to get a stable baseline identity in each
// batch for comparison. Left blank in this release (was our own test handle); set it to a
// `mailto:`/`tel:` handle to re-enable, or leave empty to send chunks without a baseline handle.
const ALWAYS_INCLUDE_HANDLE: &str = "";

#[derive(Deserialize)]
struct HwInfo {
    #[serde(rename = "os_config")]
    os_config: MacOSConfig,
    push: APSState,
    identity: IDSNGMIdentity,
}

fn print_usage(program: &str) {
    eprintln!(
        "Usage: {program} <bluebubbles_dir> <sqlite_db_path> [--handles-file <path>] [--chunk-size <n>] [--chunk-timeout-seconds <n>] [--random] [handle ...]\n\n\
         Examples:\n  {program} ~/BlueBubbles data/ids_lookup.db\n  {program} ~/BlueBubbles lookup.db --handles-file handles.json --chunk-size 500\n  {program} ~/BlueBubbles lookup.db --chunk-timeout-seconds 2 mailto:example@icloud.com\n  {program} ~/BlueBubbles lookup.db --chunk-size 100 --random"
    );
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    if env::var("RUST_LOG").is_err() {
        env::set_var("RUST_LOG", "info");
    }
    pretty_env_logger::try_init().ok();

    let mut args = env::args();
    let program = args.next().unwrap_or_else(|| "imessage-query".to_string());
    let bluebubbles_dir = match args.next() {
        Some(value) => PathBuf::from(value),
        None => {
            print_usage(&program);
            return Ok(());
        }
    };
    let db_path = match args.next() {
        Some(value) => PathBuf::from(value),
        None => {
            print_usage(&program);
            return Ok(());
        }
    };

    let mut handles: Vec<String> = Vec::new();
    let mut handles_file: Option<PathBuf> = None;
    let mut chunk_size: Option<usize> = None;
    let mut chunk_timeout_secs: Option<u64> = None;
    let mut randomize_chunk_size = false;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--handles-file" => {
                let Some(path) = args.next() else {
                    eprintln!("--handles-file requires a file path");
                    return Ok(());
                };
                handles_file = Some(PathBuf::from(path));
            }
            "--chunk-size" => {
                let Some(size) = args.next() else {
                    eprintln!("--chunk-size requires a numeric value");
                    return Ok(());
                };
                match size.parse::<usize>() {
                    Ok(value) if value > 0 => chunk_size = Some(value),
                    Ok(_) | Err(_) => {
                        eprintln!("--chunk-size must be a positive integer");
                        return Ok(());
                    }
                }
            }
            "--chunk-timeout-seconds" => {
                let Some(timeout) = args.next() else {
                    eprintln!("--chunk-timeout-seconds requires a numeric value (seconds)");
                    return Ok(());
                };
                match timeout.parse::<u64>() {
                    Ok(value) => chunk_timeout_secs = Some(value),
                    Err(_) => {
                        eprintln!("--chunk-timeout-seconds must be a positive integer");
                        return Ok(());
                    }
                }
            }
            // Vary the number of handles per chunk instead of always sending exactly
            // --chunk-size, to make the query traffic pattern less uniform/fingerprintable.
            "--random" => randomize_chunk_size = true,
            "--" => {
                handles.extend(args);
                break;
            }
            value if value.starts_with("--") => {
                eprintln!("Unknown option: {value}");
                print_usage(&program);
                return Ok(());
            }
            value => handles.push(value.to_string()),
        }
    }

    if let Some(path) = handles_file {
        debug!("Loading handles from {}", path.display());
        let data = fs::read(&path).await?;
        let mut from_file: Vec<String> = serde_json::from_slice(&data)?;
        handles.append(&mut from_file);
    }

    let mut seen = HashSet::new();
    handles.retain(|handle| seen.insert(handle.clone()));

    let user_provided_handles = !handles.is_empty();

    if user_provided_handles {
        info!("Preparing to query {} handle(s) from input", handles.len());
    } else {
        info!("No explicit handles provided; will query default self handle");
    }

    if let Some(parent) = db_path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent).await?;
        }
    }

    let db_preexisting = db_path.exists();
    QueryClient::initialize_database(&db_path).await?;
    if !db_preexisting {
        info!(
            "Initialized SQLite store at {}; no prior database found",
            db_path.display()
        );
    }

    let hw_info_path = bluebubbles_dir.join("hw_info.plist");
    let hw_info_data = fs::read(&hw_info_path).await?;
    let hw_info: HwInfo = plist::from_bytes(&hw_info_data)?;

    let config = Arc::new(hw_info.os_config);
    let push_state = hw_info.push;
    let identity = hw_info.identity;

    let users_path = bluebubbles_dir.join("id.plist");
    let users_data = fs::read(&users_path).await?;
    let users: Vec<IDSUser> = plist::from_bytes(&users_data)?;

    let (connection, connection_error) =
        APSConnectionResource::new(config.clone(), Some(push_state)).await;
    if let Some(error) = connection_error {
        return Err(format!("Failed to create APS connection: {error}").into());
    }

    let services = &[&MADRID_SERVICE];
    let cache_path =
        std::env::temp_dir().join(format!("rustpush-id-cache-{}.plist", Uuid::new_v4()));

    let client = IMClient::new(
        connection.clone(),
        users,
        identity,
        services,
        cache_path.clone(),
        config.clone(),
        Box::new(|_| {}),
    )
    .await;

    let mut query_client = QueryClient::new(client, &MADRID_SERVICE).await?;
    let chunk_size = chunk_size.unwrap_or(5000);
    query_client.set_chunk_size(chunk_size);

    let chunk_timeout = chunk_timeout_secs.map(Duration::from_secs);

    let existing_handles = load_existing_handles(&db_path).await?;

    let mut handles_to_process = if user_provided_handles {
        handles
    } else {
        vec![query_client.self_handle().to_string()]
    };

    if !handles_to_process
        .iter()
        .any(|handle| handle == ALWAYS_INCLUDE_HANDLE)
    {
        handles_to_process.push(ALWAYS_INCLUDE_HANDLE.to_string());
    }

    let before_filter = handles_to_process.len();
    handles_to_process
        .retain(|handle| handle == ALWAYS_INCLUDE_HANDLE || !existing_handles.contains(handle));
    let skipped_existing = before_filter.saturating_sub(handles_to_process.len());

    if skipped_existing > 0 {
        info!(
            "Skipping {} handle(s) already present in database; {} remaining to query (excluding always-included handle)",
            skipped_existing,
            handles_to_process.len()
        );
    }

    // Remove the always-included handle from the regular pool so we can append it to every chunk.
    let always_included_present = handles_to_process
        .iter()
        .any(|handle| handle == ALWAYS_INCLUDE_HANDLE);
    handles_to_process.retain(|handle| handle != ALWAYS_INCLUDE_HANDLE);

    if handles_to_process.is_empty() && !always_included_present {
        warn!("No handles available for querying; exiting");
        return Ok(());
    }

    // Reserve one slot per chunk for the always-included handle.
    let chunk_capacity = if chunk_size > 1 { chunk_size - 1 } else { 1 };
    let chunk_plans = build_chunk_plan(
        &handles_to_process,
        chunk_capacity,
        chunk_size,
        randomize_chunk_size,
    );
    let total_chunks = chunk_plans.len();

    if randomize_chunk_size && chunk_size > 1 {
        info!(
            "Random chunk sizing enabled: will query between 2 and {} handles per chunk (including always-included handle)",
            chunk_size
        );
    }
    let mut total_handle_count = 0usize;
    let mut total_identity_count = 0usize;

    for (chunk_idx, chunk) in chunk_plans.into_iter().enumerate() {
        let mut attempt = 1usize;
        loop {
            let mut chunk_vec = chunk.clone();
            if !chunk_vec.iter().any(|h| h == ALWAYS_INCLUDE_HANDLE) || chunk_vec.is_empty() {
                if chunk_size > 1 && chunk_vec.len() >= chunk_size {
                    chunk_vec.pop();
                }
                chunk_vec.push(ALWAYS_INCLUDE_HANDLE.to_string());
            }

            info!(
                "Processing chunk {}/{} with {} handle(s) (attempt {})",
                chunk_idx + 1,
                total_chunks,
                chunk_vec.len(),
                attempt
            );

            let options = QueryOptions::default();
            let results = query_client
                .query_handles(Some(&chunk_vec), options)
                .await?;
            let identity_count: usize = results.iter().map(|r| r.identities.len()).sum();

            for result in &results {
                info!(
                    "Handle {}: {} identit(y/ies), status {:?}",
                    result.handle,
                    result.identities.len(),
                    result.status
                );
            }

            // A chunk returning zero identities usually means IDS is rate-limiting this
            // identity rather than that every handle in the chunk is unregistered; back off
            // and retry the same chunk instead of treating it as "no results".
            if identity_count == 0 {
                warn!(
                    "Chunk {}/{} returned no identities; sleeping 60s before retrying",
                    chunk_idx + 1,
                    total_chunks
                );
                sleep(Duration::from_secs(60)).await;
                attempt += 1;
                continue;
            }

            let summary = query_client.store_results(&db_path, &results).await?;
            total_handle_count += summary.handle_count;
            total_identity_count += summary.identity_count;

            info!(
                "Stored lookup data for {} handle(s) and {} identity record(s) from chunk {}/{} in {}",
                summary.handle_count,
                summary.identity_count,
                chunk_idx + 1,
                total_chunks,
                db_path.display()
            );

            if let Some(timeout) = chunk_timeout {
                if !timeout.is_zero() && chunk_idx + 1 < total_chunks {
                    info!(
                        "Waiting {}s before proceeding to the next chunk",
                        timeout.as_secs()
                    );
                    sleep(timeout).await;
                }
            }

            break;
        }
    }

    info!(
        "Finished querying {} handle(s) across {} chunk(s); stored {} identities",
        total_handle_count, total_chunks, total_identity_count
    );

    if !user_provided_handles {
        warn!("Consider providing explicit handles for broader coverage");
    }

    if let Err(err) = fs::remove_file(&cache_path).await {
        if err.kind() != std::io::ErrorKind::NotFound {
            warn!(
                "Failed to remove temporary cache file {}: {}",
                cache_path.display(),
                err
            );
        }
    }

    Ok(())
}

fn build_chunk_plan(
    handles: &[String],
    chunk_capacity: usize,
    chunk_size: usize,
    randomize_chunk_size: bool,
) -> Vec<Vec<String>> {
    let mut plans: Vec<Vec<String>> = Vec::new();
    if handles.is_empty() {
        return plans;
    }

    let mut rng = thread_rng();
    let mut start = 0usize;

    while start < handles.len() {
        let mut capacity = chunk_capacity.max(1);
        if randomize_chunk_size && chunk_size > 1 {
            let target_total = rng.gen_range(2..=chunk_size);
            capacity = capacity.min(target_total.saturating_sub(1).max(1));
        }
        let end = (start + capacity).min(handles.len());
        plans.push(handles[start..end].to_vec());
        start = end;
    }

    plans
}

async fn load_existing_handles(
    db_path: &Path,
) -> Result<HashSet<String>, Box<dyn std::error::Error + Send + Sync>> {
    let path = db_path.to_path_buf();
    let handles = tokio::task::spawn_blocking(
        move || -> Result<HashSet<String>, Box<dyn std::error::Error + Send + Sync>> {
            let conn = Connection::open(path)?;
            let mut stmt = conn.prepare("SELECT handle FROM handles")?;
            let mut existing = HashSet::new();
            let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
            for row in rows {
                existing.insert(row?);
            }
            Ok(existing)
        },
    )
    .await??;

    Ok(handles)
}
