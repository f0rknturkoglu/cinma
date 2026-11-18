//! Opt-in local Unix domain socket control interface (`<data_dir>/cinma.sock`, #N25).
//! Disabled by default; passively awaits connections (`0%` idle CPU) and only lives inside Cinma.

use std::sync::Arc;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixListener;

use crate::{Core, CoreError};

impl Core {
    pub fn local_api_socket_path(&self) -> String {
        self.data_dir
            .join("cinma.sock")
            .to_string_lossy()
            .into_owned()
    }

    pub fn sync_local_api_socket(self: &Arc<Self>) -> Result<(), CoreError> {
        let enabled = self
            .player_preferences()
            .map(|p| p.local_api_socket_enabled)
            .unwrap_or(false);
        let sock_path = self.data_dir.join("cinma.sock");

        let mut slot = self
            .local_api_cancel
            .lock()
            .map_err(|_| CoreError::Poisoned)?;
        if let Some(prev) = slot.take() {
            prev.cancel();
            let _ = std::fs::remove_file(&sock_path);
        }
        if !enabled {
            return Ok(());
        }

        let _ = std::fs::remove_file(&sock_path);
        let token = cinma_blobstore::CancellationToken::new();
        *slot = Some(token.clone());
        drop(slot);

        let rt = self.runtime()?;
        let core = Arc::clone(self);
        rt.spawn(async move {
            let Ok(listener) = UnixListener::bind(&sock_path) else {
                return;
            };
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ =
                    std::fs::set_permissions(&sock_path, std::fs::Permissions::from_mode(0o600));
            }
            loop {
                tokio::select! {
                    biased;
                    () = token.cancelled() => {
                        let _ = std::fs::remove_file(&sock_path);
                        break;
                    }
                    accepted = listener.accept() => {
                        let Ok((stream, _)) = accepted else { break; };
                        let c = Arc::clone(&core);
                        tokio::spawn(async move {
                            handle_socket_client(c, stream).await;
                        });
                    }
                }
            }
        });
        Ok(())
    }
}

async fn handle_socket_client(core: Arc<Core>, stream: tokio::net::UnixStream) {
    let (read_half, mut write_half) = stream.into_split();
    let mut reader = BufReader::new(read_half);
    let mut line = String::new();
    while reader.read_line(&mut line).await.unwrap_or(0) > 0 {
        let trimmed = line.trim();
        if !trimmed.is_empty() {
            let reply = dispatch_rpc(&core, trimmed);
            let mut out = serde_json::to_string(&reply).unwrap_or_else(|_| "{}".to_owned());
            out.push('\n');
            if write_half.write_all(out.as_bytes()).await.is_err() {
                break;
            }
        }
        line.clear();
    }
}

fn dispatch_rpc(core: &Arc<Core>, raw: &str) -> serde_json::Value {
    let Ok(req) = serde_json::from_str::<serde_json::Value>(raw) else {
        return serde_json::json!({ "error": "invalid JSON" });
    };
    let id = req.get("id").cloned().unwrap_or(serde_json::Value::Null);
    let method = req.get("method").and_then(|v| v.as_str()).unwrap_or("");
    let params = req
        .get("params")
        .cloned()
        .unwrap_or(serde_json::Value::Null);

    let result = match method {
        "tools/list" => Ok(serde_json::json!({
            "tools": [
                { "name": "status", "description": "Get Cinma version and queue status" },
                { "name": "downloads.list", "description": "List queued, active and completed downloads" },
                { "name": "library.list", "description": "List saved library items" },
                { "name": "continue_watching", "description": "List active Continue Watching progress" },
                { "name": "downloads.add_http", "description": "Queue a direct HTTP/HTTPS download (title, url)" },
                { "name": "downloads.add_magnet", "description": "Queue a magnet download (title, uri)" }
            ]
        })),
        "tools/call" => {
            let tool = params.get("name").and_then(|v| v.as_str()).unwrap_or("");
            let args = params
                .get("arguments")
                .cloned()
                .unwrap_or(serde_json::Value::Null);
            call_tool(core, tool, &args)
        }
        other => call_tool(core, other, &params),
    };

    match result {
        Ok(val) => serde_json::json!({ "id": id, "result": val }),
        Err(msg) => serde_json::json!({ "id": id, "error": msg }),
    }
}

fn call_tool(
    core: &Arc<Core>,
    tool: &str,
    args: &serde_json::Value,
) -> Result<serde_json::Value, String> {
    match tool {
        "status" => {
            let dl_count = core.downloads().map(|d| d.len()).unwrap_or(0);
            let lib_count = core.library_items().map(|l| l.len()).unwrap_or(0);
            Ok(serde_json::json!({
                "app": "Cinma",
                "version": crate::VERSION,
                "downloads": dl_count,
                "library_items": lib_count
            }))
        }
        "downloads.list" => {
            let rows = core.downloads().map_err(|e| e.to_string())?;
            let list: Vec<_> = rows
                .into_iter()
                .map(|r| {
                    serde_json::json!({
                        "id": r.id,
                        "title": r.title,
                        "kind": r.kind.as_str(),
                        "state": r.state.as_str(),
                        "bytes_done": r.bytes_done,
                        "bytes_total": r.bytes_total
                    })
                })
                .collect();
            Ok(serde_json::json!(list))
        }
        "library.list" => {
            let rows = core.library_items().map_err(|e| e.to_string())?;
            let list: Vec<_> = rows
                .into_iter()
                .map(|r| {
                    serde_json::json!({
                        "meta_id": r.meta_id,
                        "media_type": r.media_type,
                        "name": r.name
                    })
                })
                .collect();
            Ok(serde_json::json!(list))
        }
        "continue_watching" => {
            let rows = core.continue_watching(20).map_err(|e| e.to_string())?;
            let list: Vec<_> = rows
                .into_iter()
                .map(|r| {
                    serde_json::json!({
                        "video_id": r.video_id,
                        "meta_id": r.meta_id,
                        "title": r.title,
                        "position_s": r.position_s,
                        "duration_s": r.duration_s
                    })
                })
                .collect();
            Ok(serde_json::json!(list))
        }
        "downloads.add_http" => {
            let url = args
                .get("url")
                .and_then(|v| v.as_str())
                .ok_or_else(|| "missing `url`".to_owned())?;
            let title = args
                .get("title")
                .and_then(|v| v.as_str())
                .unwrap_or("Download")
                .to_owned();
            let id = core
                .add_http_download(
                    title,
                    url.to_owned(),
                    std::collections::HashMap::new(),
                    None,
                    None,
                    10,
                )
                .map_err(|e| e.to_string())?;
            Ok(serde_json::json!({ "id": id }))
        }
        "downloads.add_magnet" => {
            let uri = args
                .get("uri")
                .and_then(|v| v.as_str())
                .ok_or_else(|| "missing `uri`".to_owned())?;
            let stream = core.parse_magnet_stream(uri).map_err(|e| e.to_string())?;
            let trackers = stream.trackers();
            let title = args
                .get("title")
                .and_then(|v| v.as_str())
                .map(str::to_owned)
                .or(stream.name)
                .unwrap_or_else(|| "Magnet".to_owned());
            let hash = stream
                .info_hash
                .ok_or_else(|| "missing info_hash".to_owned())?;
            let id = core
                .add_torrent_download(title, hash, None, trackers, None, None, 10)
                .map_err(|e| e.to_string())?;
            Ok(serde_json::json!({ "id": id }))
        }
        _ => Err(format!("unknown method `{tool}`")),
    }
}
