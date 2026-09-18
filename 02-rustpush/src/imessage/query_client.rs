use std::collections::HashMap;
use std::path::Path;

use base64::{engine::general_purpose, Engine as _};
use log::debug;
use plist::Value;
use rusqlite::{params, Connection, ErrorCode};
use serde_json::Value as JsonValue;
use std::fs;
use thiserror::Error;

use crate::ids::user::{IDSService, QueryOptions};
use crate::imessage::aps_client::IMClient;
use crate::PushError;

#[derive(Debug, Clone)]
pub struct QueryResult {
    pub handle: String,
    pub status: Option<u64>,
    pub identities: Vec<Value>,
}

#[derive(Debug)]
pub struct StoreSummary {
    pub handle_count: usize,
    pub identity_count: usize,
}

#[derive(Debug, Error)]
pub enum QueryClientError {
    #[error("no handles available for the active account")]
    NoSelfHandle,
    #[error("unknown self handle '{0}'")]
    UnknownSelfHandle(String),
    #[error(transparent)]
    Push(#[from] PushError),
    #[error(transparent)]
    Database(#[from] rusqlite::Error),
    #[error(transparent)]
    Plist(#[from] plist::Error),
    #[error(transparent)]
    Utf8(#[from] std::string::FromUtf8Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    Join(#[from] tokio::task::JoinError),
}

struct StoreHandleRow {
    handle: String,
    status: Option<u64>,
    identities: Vec<StoreIdentityRow>,
}

struct StoreIdentityRow {
    index: usize,
    raw_plist_xml: String,
    json_repr: String,
}

fn ensure_schema(conn: &Connection) -> Result<(), rusqlite::Error> {
    conn.execute_batch(
        r#"
        PRAGMA journal_mode=WAL;
        PRAGMA foreign_keys=ON;
        CREATE TABLE IF NOT EXISTS handles (
            handle TEXT PRIMARY KEY,
            status INTEGER,
            last_seen DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
        );
        CREATE TABLE IF NOT EXISTS identities (
            handle TEXT NOT NULL,
            identity_index INTEGER NOT NULL,
            raw_plist TEXT NOT NULL,
            json_data TEXT NOT NULL,
            last_seen DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
            PRIMARY KEY(handle, identity_index),
            FOREIGN KEY(handle) REFERENCES handles(handle) ON DELETE CASCADE
        );
        CREATE TABLE IF NOT EXISTS query_runs (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            executed_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
            self_handle TEXT NOT NULL,
            handle_count INTEGER NOT NULL,
            identity_count INTEGER NOT NULL
        );
        "#,
    )
}

fn open_or_recreate(path: &Path) -> Result<Connection, QueryClientError> {
    match Connection::open(path) {
        Ok(conn) => Ok(conn),
        Err(rusqlite::Error::SqliteFailure(err, _)) if err.code == ErrorCode::NotADatabase => {
            let _ = fs::remove_file(path);
            Ok(Connection::open(path)?)
        }
        Err(err) => Err(err.into()),
    }
}

pub struct QueryClient {
    client: IMClient,
    service: &'static IDSService,
    self_handle: String,
    chunk_size: usize,
}

impl QueryClient {
    pub async fn new(
        client: IMClient,
        service: &'static IDSService,
    ) -> Result<Self, QueryClientError> {
        let handles = client.identity.resource.get_handles().await;
        let Some(default_handle) = handles.first().cloned() else {
            return Err(QueryClientError::NoSelfHandle);
        };
        Ok(Self {
            client,
            service,
            self_handle: default_handle,
            chunk_size: 5000,
        })
    }

    pub fn self_handle(&self) -> &str {
        &self.self_handle
    }

    pub fn chunk_size(&self) -> usize {
        self.chunk_size
    }

    pub fn set_chunk_size(&mut self, size: usize) {
        self.chunk_size = size.max(1);
    }

    pub async fn set_self_handle(&mut self, handle: String) -> Result<(), QueryClientError> {
        let handles = self.client.identity.resource.get_handles().await;
        if handles.contains(&handle) {
            self.self_handle = handle;
            Ok(())
        } else {
            Err(QueryClientError::UnknownSelfHandle(handle))
        }
    }

    pub async fn query_handles(
        &self,
        handles: Option<&[String]>,
        options: QueryOptions,
    ) -> Result<Vec<QueryResult>, QueryClientError> {
        let mut targets = Vec::new();
        if let Some(list) = handles {
            targets.extend(list.iter().cloned());
        }
        if targets.is_empty() {
            targets.push(self.self_handle.clone());
        }

        let mut aggregated: HashMap<String, QueryResult> = targets
            .iter()
            .cloned()
            .map(|handle| QueryResult {
                handle,
                status: None,
                identities: Vec::new(),
            })
            .map(|result| (result.handle.clone(), result))
            .collect();

        let config = self.client.os_config();
        let user = {
            let users = self.client.identity.resource.users.read().await;
            self.client
                .identity
                .resource
                .user_by_handle(self.service.name, &users, &self.self_handle)
                .await?
                .clone()
        };

        for chunk in targets.chunks(self.chunk_size) {
            debug!("Querying chunk of {} handles", chunk.len());
            let chunk_vec = chunk.to_vec();
            let plist = user
                .query_return_plist(
                    config.as_ref(),
                    &self.client.conn,
                    self.service.name,
                    self.service.name,
                    &self.self_handle,
                    &chunk_vec,
                    &options,
                )
                .await?;

            let results_dict = plist
                .as_dictionary()
                .and_then(|dict| dict.get("results"))
                .and_then(|value| value.as_dictionary());

            if let Some(results_dict) = results_dict {
                for handle in &chunk_vec {
                    if let Some(entry) = results_dict.get(handle) {
                        let status = entry
                            .as_dictionary()
                            .and_then(|dict| dict.get("status"))
                            .and_then(|v| v.as_unsigned_integer());
                        let identities = entry
                            .as_dictionary()
                            .and_then(|dict| dict.get("identities"))
                            .and_then(|value| value.as_array())
                            .cloned()
                            .unwrap_or_default();

                        if let Some(result) = aggregated.get_mut(handle) {
                            result.status = status;
                            result.identities = identities;
                        }
                    }
                }
            }
        }

        let results = targets
            .into_iter()
            .filter_map(|handle| aggregated.remove(&handle))
            .collect();
        Ok(results)
    }

    pub async fn store_results<P: AsRef<Path>>(
        &self,
        path: P,
        results: &[QueryResult],
    ) -> Result<StoreSummary, QueryClientError> {
        let path = path.as_ref().to_path_buf();
        let self_handle = self.self_handle.clone();

        let mut rows = Vec::with_capacity(results.len());
        for result in results {
            let mut identities = Vec::with_capacity(result.identities.len());
            for (idx, value) in result.identities.iter().enumerate() {
                let mut buf = Vec::new();
                plist::to_writer_xml(&mut buf, value)?;
                let xml = String::from_utf8(buf)?;
                let json = serde_json::to_string(&plist_to_json(value))?;
                identities.push(StoreIdentityRow {
                    index: idx,
                    raw_plist_xml: xml,
                    json_repr: json,
                });
            }
            rows.push(StoreHandleRow {
                handle: result.handle.clone(),
                status: result.status,
                identities,
            });
        }

        let handle_count = rows.len();
        let identity_count: usize = rows.iter().map(|row| row.identities.len()).sum();

        tokio::task::spawn_blocking(move || -> Result<(), QueryClientError> {
            let mut conn = open_or_recreate(&path)?;
            ensure_schema(&conn)?;

            let tx = conn.transaction()?;
            tx.execute(
                "INSERT INTO query_runs (self_handle, handle_count, identity_count) VALUES (?1, ?2, ?3)",
                params![self_handle.as_str(), handle_count as i64, identity_count as i64],
            )?;

            for row in &rows {
                tx.execute(
                    "INSERT INTO handles (handle, status, last_seen) VALUES (?1, ?2, CURRENT_TIMESTAMP)
                     ON CONFLICT(handle) DO UPDATE SET status=excluded.status, last_seen=CURRENT_TIMESTAMP",
                    params![row.handle.as_str(), row.status.map(|s| s as i64)],
                )?;
                tx.execute("DELETE FROM identities WHERE handle = ?1", params![row.handle.as_str()])?;
                for identity in &row.identities {
                    tx.execute(
                        "INSERT INTO identities (handle, identity_index, raw_plist, json_data, last_seen)
                         VALUES (?1, ?2, ?3, ?4, CURRENT_TIMESTAMP)",
                        params![
                            row.handle.as_str(),
                            identity.index as i64,
                            identity.raw_plist_xml.as_str(),
                            identity.json_repr.as_str()
                        ],
                    )?;
                }
            }

            tx.commit()?;
            Ok(())
        })
        .await??;

        Ok(StoreSummary {
            handle_count,
            identity_count,
        })
    }

    pub async fn initialize_database<P: AsRef<Path>>(path: P) -> Result<(), QueryClientError> {
        let path = path.as_ref().to_path_buf();
        tokio::task::spawn_blocking(move || -> Result<(), QueryClientError> {
            let mut conn = open_or_recreate(&path)?;
            ensure_schema(&conn)?;
            Ok(())
        })
        .await??;
        Ok(())
    }

    pub async fn query_and_store<P: AsRef<Path>>(
        &self,
        handles: Option<&[String]>,
        options: QueryOptions,
        path: P,
    ) -> Result<(Vec<QueryResult>, StoreSummary), QueryClientError> {
        let results = self.query_handles(handles, options).await?;
        let summary = self.store_results(path, &results).await?;
        Ok((results, summary))
    }

    pub fn client(&self) -> &IMClient {
        &self.client
    }
}

fn plist_to_json(value: &Value) -> JsonValue {
    match value {
        Value::String(s) => JsonValue::String(s.clone()),
        Value::Integer(i) => match i.to_string().parse::<i64>() {
            Ok(parsed) => JsonValue::Number(parsed.into()),
            Err(_) => JsonValue::String(i.to_string()),
        },
        Value::Real(r) => serde_json::Number::from_f64(*r)
            .map(JsonValue::Number)
            .unwrap_or(JsonValue::Null),
        Value::Boolean(b) => JsonValue::Bool(*b),
        Value::Array(arr) => JsonValue::Array(arr.iter().map(plist_to_json).collect()),
        Value::Dictionary(dict) => {
            let mut map = serde_json::Map::new();
            for (key, val) in dict {
                map.insert(key.clone(), plist_to_json(val));
            }
            JsonValue::Object(map)
        }
        Value::Data(data) => JsonValue::String(general_purpose::STANDARD.encode(data.as_slice())),
        Value::Date(date) => JsonValue::String(format!("{:?}", date)),
        Value::Uid(uid) => JsonValue::String(format!("{}", uid.get())),
        _ => JsonValue::Null,
    }
}
