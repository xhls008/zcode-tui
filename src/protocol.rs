//! Official app-server and V4 protocol shapes, reducers, and stdio client.

use super::*;

/// Machine-readable event emitted by the classic `zcode --json` path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StreamEvent {
    ToolUse { name: String, detail: String },
    ToolResult { detail: String },
    Text(String),
    Meta(String),
}

/// Recognize one line of classic machine-readable agent output. Plain text
/// returns `None` so the caller can preserve the raw fallback behavior.
pub fn parse_stream_event(line: &str) -> Option<StreamEvent> {
    let trimmed = line.trim();
    if !trimmed.starts_with('{') || !trimmed.ends_with('}') {
        return None;
    }
    let value: serde_json::Value = serde_json::from_str(trimmed).ok()?;
    let object = value.as_object()?;
    let event_type = object.get("type").and_then(|t| t.as_str())?;

    let text_of = |keys: &[&str]| -> Option<String> {
        for key in keys {
            match object.get(*key) {
                Some(serde_json::Value::String(text)) => return Some(text.clone()),
                Some(serde_json::Value::Array(parts)) => {
                    let joined: String = parts
                        .iter()
                        .filter_map(|part| {
                            part.get("text")
                                .and_then(|t| t.as_str())
                                .or_else(|| part.as_str())
                        })
                        .collect::<Vec<_>>()
                        .join("");
                    if !joined.is_empty() {
                        return Some(joined);
                    }
                }
                _ => {}
            }
        }
        None
    };

    if event_type.contains("tool_use") || event_type == "tool_call" {
        let name = text_of(&["name", "tool", "tool_name"]).unwrap_or_else(|| "tool".to_string());
        let detail = object
            .get("input")
            .or_else(|| object.get("arguments"))
            .or_else(|| object.get("args"))
            .map(|v| compact_json(v, 80))
            .unwrap_or_default();
        return Some(StreamEvent::ToolUse { name, detail });
    }
    if event_type.contains("tool_result") || event_type.contains("tool_output") {
        let detail = text_of(&["content", "output", "result"])
            .map(|text| truncate_chars(&text, 80))
            .unwrap_or_else(|| "done".to_string());
        return Some(StreamEvent::ToolResult { detail });
    }
    if matches!(event_type, "text" | "message" | "assistant" | "completion") {
        if let Some(text) = text_of(&["text", "content", "message"]) {
            return Some(StreamEvent::Text(text));
        }
    }
    Some(StreamEvent::Meta(event_type.to_string()))
}

fn compact_json(value: &serde_json::Value, max: usize) -> String {
    let rendered = match value {
        serde_json::Value::String(text) => text.clone(),
        other => other.to_string(),
    };
    truncate_chars(&rendered.replace('\n', " "), max)
}

fn truncate_chars(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let kept: String = text.chars().take(max).collect();
    format!("{kept}…")
}

// ---- app-server protocol client (default-on, ZCODE_TUI_APP_SERVER=0 opts out) ----
//
// The kernel's `app-server` is a newline-delimited JSON stdio protocol
// (envelope {id, method, params}, NOT JSON-RPC — a `jsonrpc` key is
// rejected). It is the only path to true token streaming: `--prompt`
// buffers the kernel's internal delta stream, but app-server re-exposes it.
//
// Verified sequence (2026-07-06):
//   session/create {workspace:{workspaceKey, workspacePath}}  -> session.sessionId
//   session/subscribe {sessionId, deliveryKind:"desktop-continuous"}
//   session/send {sessionId, content}                         -> {accepted:true}
//   <- session/event notifications: params.payload.{kind, delta, done}
//      kind text_delta carries the streamed body token by token.

/// deliveryKind that streams events continuously (vs web-remote-replayable).
pub const APP_SERVER_DELIVERY_KIND: &str = "desktop-continuous";

/// Encode one request as a single compact JSON line (no jsonrpc field, no
/// trailing newline — the caller frames with `\n`).
pub fn encode_app_request(id: u64, method: &str, params: serde_json::Value) -> String {
    serde_json::json!({ "id": id, "method": method, "params": params }).to_string()
}

pub fn app_create_params(workspace_path: &str) -> serde_json::Value {
    serde_json::json!({
        "workspace": { "workspaceKey": workspace_path, "workspacePath": workspace_path }
    })
}

pub fn app_subscribe_params(session_id: &str) -> serde_json::Value {
    serde_json::json!({
        "sessionId": session_id,
        "deliveryKind": APP_SERVER_DELIVERY_KIND,
        "includeSnapshot": true
    })
}

pub fn app_send_params(session_id: &str, content: &str) -> serde_json::Value {
    serde_json::json!({ "sessionId": session_id, "content": content })
}

/// `session/send` with `attachments[]`. Empty attachments MUST yield the
/// exact same shape as [`app_send_params`] (no `attachments` key), so the
/// no-mention path stays byte-identical to the pre-attachment behaviour.
pub fn app_send_params_with_attachments(
    session_id: &str,
    content: &str,
    attachments: &[serde_json::Value],
) -> serde_json::Value {
    if attachments.is_empty() {
        return app_send_params(session_id, content);
    }
    serde_json::json!({
        "sessionId": session_id,
        "content": content,
        "attachments": attachments,
    })
}

/// Extensions the kernel treats as images (attachment `kind:"image"`).
fn image_mime_for(ext: &str) -> Option<&'static str> {
    match ext {
        "png" => Some("image/png"),
        "jpg" | "jpeg" => Some("image/jpeg"),
        "gif" => Some("image/gif"),
        "webp" => Some("image/webp"),
        _ => None,
    }
}

/// Best-effort mimeType for `kind:"file"` attachments; unknown extensions
/// fall back to text/plain (the kernel only needs a plausible type).
fn file_mime_for(ext: &str) -> &'static str {
    match ext {
        "md" | "markdown" => "text/markdown",
        "json" => "application/json",
        "yaml" | "yml" => "application/yaml",
        "toml" => "application/toml",
        "xml" => "application/xml",
        "html" | "htm" => "text/html",
        "css" => "text/css",
        "csv" => "text/csv",
        "pdf" => "application/pdf",
        "zip" => "application/zip",
        "svg" => "image/svg+xml",
        "js" | "mjs" => "text/javascript",
        _ => "text/plain",
    }
}

/// Map `@file` mentions (already vetted by [`extract_file_mentions`]) to
/// `session/send` attachment objects — kernel bundle schema `Pwt`, a strict
/// union discriminated on `kind`, pinned live 2026-07-07 on kernel 0.15.0:
/// image `{kind, filename, mimeType, sizeBytes?, dataBase64?, localPath?}`,
/// file `{kind, filename, mimeType, sizeBytes(REQUIRED), dataBase64?,
/// textContent?, localPath?}`. `localPath` alone is sufficient (verified:
/// the model reads the referenced file's content); `sizeBytes` is mandatory
/// for `kind:"file"`, so a mention whose metadata cannot be read is skipped
/// rather than sent half-formed (a strict-schema ZodError would kill the
/// whole send).
pub fn build_send_attachments(mentions: &[String], cwd: &Path) -> Vec<serde_json::Value> {
    let mut attachments = Vec::new();
    for mention in mentions {
        let Ok(resolved) = cwd.join(mention).canonicalize() else {
            continue;
        };
        let Ok(meta) = fs::metadata(&resolved) else {
            continue;
        };
        let filename = resolved
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| mention.clone());
        let ext = resolved
            .extension()
            .map(|ext| ext.to_string_lossy().to_ascii_lowercase())
            .unwrap_or_default();
        let local_path = resolved.to_string_lossy().into_owned();
        let attachment = match image_mime_for(&ext) {
            Some(mime) => serde_json::json!({
                "kind": "image",
                "filename": filename,
                "mimeType": mime,
                "sizeBytes": meta.len(),
                "localPath": local_path,
            }),
            None => serde_json::json!({
                "kind": "file",
                "filename": filename,
                "mimeType": file_mime_for(&ext),
                "sizeBytes": meta.len(),
                "localPath": local_path,
            }),
        };
        attachments.push(attachment);
    }
    attachments
}

pub fn app_stop_params(session_id: &str) -> serde_json::Value {
    serde_json::json!({ "sessionId": session_id })
}

// Session-control params (schemas pinned 2026-07-07 via zod-error probing on
// kernel 0.15.0; setMode verified live).

/// `session/setMode` — mode ∈ plan|build|edit|yolo|auto (kernel-enforced enum).
pub fn app_set_mode_params(session_id: &str, mode: &str) -> serde_json::Value {
    serde_json::json!({ "sessionId": session_id, "mode": mode })
}

/// `session/setModel` — `model` is sent back verbatim from the state push's
/// `model.available[].ref` (shape `{modelId, providerId}`).
pub fn app_set_model_params(session_id: &str, model_ref: &serde_json::Value) -> serde_json::Value {
    serde_json::json!({ "sessionId": session_id, "model": model_ref })
}

/// `session/setThoughtLevel` — level values per the state push's
/// `thoughtLevel.available[].value` (observed: enabled/disabled).
pub fn app_set_thought_params(session_id: &str, level: &str) -> serde_json::Value {
    serde_json::json!({ "sessionId": session_id, "thoughtLevel": level })
}

/// `session/compact` — compacts the session context in place.
pub fn app_compact_params(session_id: &str) -> serde_json::Value {
    serde_json::json!({ "sessionId": session_id })
}

/// `session/steer` — inject input into the RUNNING turn (same shape as send).
pub fn app_steer_params(session_id: &str, content: &str) -> serde_json::Value {
    serde_json::json!({ "sessionId": session_id, "content": content })
}

/// Subscribe to the ZCode 3.5.3 conversation control plane while keeping the
/// legacy session subscription for token/body events.
pub fn v4_conversation_subscribe_params(
    session_id: &str,
    connection_id: &str,
) -> serde_json::Value {
    serde_json::json!({
        "topic": format!("conversation/{session_id}"),
        "connectionId": connection_id,
        "clientMode": APP_SERVER_DELIVERY_KIND,
        "visibility": "foreground",
    })
}

/// Generic V4 command envelope. Only commands whose bundle schema requires a
/// CAS field receive it: setFollowupMode needs baseRevision, while
/// applyFileRewind needs both revision and log epoch. sendText deliberately
/// works without a CAS base and is judged by the semantic delivery frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum V4CommandBase<'a> {
    None,
    Revision(u64),
    RevisionAndEpoch { revision: u64, log_epoch: &'a str },
}

pub fn v4_command_params(
    command_id: &str,
    client_id: &str,
    session_id: &str,
    command_type: &str,
    payload: serde_json::Value,
    base: V4CommandBase<'_>,
    issued_at: u64,
) -> serde_json::Value {
    let mut value = serde_json::json!({
        "commandId": command_id,
        "clientId": client_id,
        "sessionId": session_id,
        "type": command_type,
        "payload": payload,
        "issuedAt": issued_at,
    });
    let map = value
        .as_object_mut()
        .expect("v4 command envelope is object");
    match base {
        V4CommandBase::None => {}
        V4CommandBase::Revision(revision) => {
            map.insert("baseRevision".to_string(), serde_json::json!(revision));
        }
        V4CommandBase::RevisionAndEpoch {
            revision,
            log_epoch,
        } => {
            map.insert("baseRevision".to_string(), serde_json::json!(revision));
            map.insert("baseLogEpoch".to_string(), serde_json::json!(log_epoch));
        }
    }
    value
}

/// V4 row identity used by preview/apply (strict `{rowId, entityId}`).
pub fn v4_rewind_target(row_id: u64, entity_id: &str) -> serde_json::Value {
    serde_json::json!({ "rowId": row_id, "entityId": entity_id })
}

pub fn v4_file_rewind_preview_params(
    session_id: &str,
    row_id: u64,
    entity_id: &str,
    base_revision: u64,
    base_log_epoch: &str,
) -> serde_json::Value {
    serde_json::json!({
        "sessionId": session_id,
        "target": v4_rewind_target(row_id, entity_id),
        "baseRevision": base_revision,
        "baseLogEpoch": base_log_epoch,
    })
}

/// A V4 conversation row reduced to the fields needed by `/rewind`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct V4ConversationRow {
    pub row_id: u64,
    pub entity_id: String,
    pub kind: String,
    pub state: String,
    pub files: u64,
    pub additions: u64,
    pub deletions: u64,
    pub file_state: Option<String>,
    pub can_rewind_files: bool,
}

fn parse_v4_row(value: &serde_json::Value) -> Option<V4ConversationRow> {
    let changes = value.get("fileChanges");
    Some(V4ConversationRow {
        row_id: value.get("rowId")?.as_u64()?,
        entity_id: value.get("entityId")?.as_str()?.to_string(),
        kind: str_at(value, "kind"),
        state: str_at(value, "state"),
        files: changes
            .and_then(|v| v.get("files"))
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0),
        additions: changes
            .and_then(|v| v.get("additions"))
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0),
        deletions: changes
            .and_then(|v| v.get("deletions"))
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0),
        file_state: changes
            .and_then(|v| v.get("state"))
            .and_then(|v| v.as_str())
            .map(str::to_string),
        can_rewind_files: value
            .pointer("/actions/canRewindFiles")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false),
    })
}

/// Minimal snapshot/delta cache for the hybrid V4 control plane. The V4
/// frame's `toSeq` is an event sequence, not the command CAS revision; only
/// `snapshot.revision` and `state.updated.patch.revision` update `revision`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct V4ConversationState {
    pub revision: Option<u64>,
    pub log_epoch: Option<String>,
    pub input_routing: Option<String>,
    pub followup_mode: Option<String>,
    pub set_followup_allowed: Option<bool>,
    pub rows: Vec<V4ConversationRow>,
    /// Command id -> admitted delivery, retained so a frame that races ahead
    /// of its response can still settle the pending steer once the ack lands.
    pub input_deliveries: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct V4FrameEffect {
    pub deliveries: Vec<(String, String)>,
}

impl V4ConversationState {
    fn apply_queue(&mut self, value: &serde_json::Value, effect: &mut V4FrameEffect) {
        let Some(items) = value.get("items").and_then(|v| v.as_array()) else {
            return;
        };
        for item in items {
            let Some(command_id) = item.get("sourceCommandId").and_then(|v| v.as_str()) else {
                continue;
            };
            let Some(delivery) = item.pointer("/delivery/admitted").and_then(|v| v.as_str()) else {
                continue;
            };
            self.input_deliveries
                .insert(command_id.to_string(), delivery.to_string());
            effect
                .deliveries
                .push((command_id.to_string(), delivery.to_string()));
        }
    }

    fn apply_state_patch(&mut self, patch: &serde_json::Value, effect: &mut V4FrameEffect) {
        if let Some(revision) = patch.get("revision").and_then(serde_json::Value::as_u64) {
            self.revision = Some(revision);
        }
        if let Some(mode) = patch.pointer("/inputRouting/mode").and_then(|v| v.as_str()) {
            self.input_routing = Some(mode.to_string());
        }
        if let Some(mode) = patch
            .pointer("/config/followupMode")
            .and_then(|v| v.as_str())
        {
            self.followup_mode = Some(mode.to_string());
        }
        if let Some(allowed) = patch
            .pointer("/availability/setFollowupMode/allowed")
            .and_then(serde_json::Value::as_bool)
        {
            self.set_followup_allowed = Some(allowed);
        }
        if let Some(queue) = patch.get("queue") {
            self.apply_queue(queue, effect);
        }
    }

    fn upsert_row(&mut self, row: V4ConversationRow) {
        if let Some(existing) = self.rows.iter_mut().find(|old| old.row_id == row.row_id) {
            *existing = row;
        } else {
            self.rows.push(row);
            self.rows.sort_by_key(|row| row.row_id);
        }
    }

    /// Apply one complete `v4/conversation/frame` params object. Fragmented
    /// transport frames are ignored safely; the conversation window is bounded
    /// and normal CLI frames are complete in observed 3.5.3 sessions.
    pub fn apply_frame(&mut self, params: &serde_json::Value) -> V4FrameEffect {
        let mut effect = V4FrameEffect::default();
        let Some(payload) = params.pointer("/frame/payload") else {
            return effect;
        };
        match payload.get("kind").and_then(|v| v.as_str()) {
            Some("snapshot") => {
                let Some(snapshot) = payload.get("snapshot") else {
                    return effect;
                };
                self.revision = snapshot.get("revision").and_then(serde_json::Value::as_u64);
                self.log_epoch = snapshot
                    .get("logEpoch")
                    .and_then(|v| v.as_str())
                    .map(str::to_string);
                self.input_routing = snapshot
                    .pointer("/inputRouting/mode")
                    .and_then(|v| v.as_str())
                    .map(str::to_string);
                self.followup_mode = snapshot
                    .pointer("/config/followupMode")
                    .and_then(|v| v.as_str())
                    .map(str::to_string);
                self.set_followup_allowed = snapshot
                    .pointer("/availability/setFollowupMode/allowed")
                    .and_then(serde_json::Value::as_bool);
                self.rows = snapshot
                    .pointer("/rows/window")
                    .and_then(|v| v.as_array())
                    .map(|rows| rows.iter().filter_map(parse_v4_row).collect())
                    .unwrap_or_default();
                self.input_deliveries.clear();
                if let Some(queue) = snapshot.get("queue") {
                    self.apply_queue(queue, &mut effect);
                }
            }
            Some("deltas") => {
                let Some(deltas) = payload.get("deltas").and_then(|v| v.as_array()) else {
                    return effect;
                };
                for delta in deltas {
                    match delta.get("op").and_then(|v| v.as_str()) {
                        Some("state.updated") => {
                            if let Some(patch) = delta.get("patch") {
                                self.apply_state_patch(patch, &mut effect);
                            }
                        }
                        Some("row.appended") | Some("row.upserted") => {
                            if let Some(row) = delta.get("row").and_then(parse_v4_row) {
                                self.upsert_row(row);
                            }
                        }
                        Some("row.removed") => {
                            if let Some(row_id) =
                                delta.get("rowId").and_then(serde_json::Value::as_u64)
                            {
                                self.rows.retain(|row| row.row_id != row_id);
                            }
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
        effect
    }

    pub fn rewind_rows(&self) -> Vec<&V4ConversationRow> {
        self.rows
            .iter()
            .rev()
            .filter(|row| {
                row.kind == "turnHeader"
                    && row.can_rewind_files
                    && row.files > 0
                    && row.file_state.as_deref() != Some("reverted")
            })
            .collect()
    }

    pub fn delivery_for(&self, command_id: &str) -> Option<&str> {
        self.input_deliveries.get(command_id).map(String::as_str)
    }
}

/// Semantic acknowledgement returned by `v4/command`. A successful response
/// envelope may still carry status stale/rejected/failed, so callers must
/// inspect this object before changing UI state.
#[derive(Debug, Clone, PartialEq)]
pub struct V4CommandAck {
    pub command_id: String,
    pub status: String,
    pub reason_code: Option<String>,
    pub message: Option<String>,
    pub revision_at_decision: u64,
    pub result: Option<serde_json::Value>,
}

impl V4CommandAck {
    pub fn accepted(&self) -> bool {
        matches!(self.status.as_str(), "accepted" | "duplicate" | "noop")
    }

    pub fn input_delivery(&self) -> Option<&str> {
        self.result
            .as_ref()
            .filter(|result| {
                result.get("type").and_then(|v| v.as_str()) == Some("inputDisposition")
            })
            .and_then(|result| result.get("delivery"))
            .and_then(|v| v.as_str())
    }
}

pub fn parse_v4_command_ack(result: &serde_json::Value) -> Option<V4CommandAck> {
    Some(V4CommandAck {
        command_id: result.get("commandId")?.as_str()?.to_string(),
        status: result.get("status")?.as_str()?.to_string(),
        reason_code: result
            .get("reasonCode")
            .and_then(|v| v.as_str())
            .map(str::to_string),
        message: result
            .get("message")
            .and_then(|v| v.as_str())
            .map(str::to_string),
        revision_at_decision: result
            .get("revisionAtDecision")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0),
        result: result.get("result").cloned(),
    })
}

/// `session/resume` — reopen an existing session; the result is shaped like
/// `session/create`'s (verified live: messages/projection/session/todos).
/// `runtime_model` (from [`build_runtime_model`]) MUST accompany the resume:
/// resume restores the conversation but NOT the model runtime — without it
/// the first send fails with `ZCODE_RUNTIME_MODEL_UNAVAILABLE` ("历史任务
/// 使用的模型已不可用", pinned live 2026-07-07).
pub fn app_resume_params(
    session_id: &str,
    runtime_model: Option<&serde_json::Value>,
) -> serde_json::Value {
    match runtime_model {
        Some(runtime) => {
            serde_json::json!({ "sessionId": session_id, "runtimeModel": runtime })
        }
        None => serde_json::json!({ "sessionId": session_id }),
    }
}

/// Attach an `mcpServers` array to `session/create`/`session/resume` params
/// (both schemas carry the same optional field). `None` leaves the params
/// untouched so kernels predating the field never see an unknown key.
pub fn with_mcp_servers(
    mut params: serde_json::Value,
    servers: Option<serde_json::Value>,
) -> serde_json::Value {
    if let (Some(list), Some(map)) = (servers, params.as_object_mut()) {
        map.insert("mcpServers".to_string(), list);
    }
    params
}

/// Attach the ZCode 3.3.6 app-server session policy fields. Both create and
/// resume use the same strict optional keys. Empty lists deliberately leave
/// the request untouched so the no-policy path stays compatible with older
/// kernels and byte-identical to the pre-adaptation shape.
pub fn with_tool_policy(
    mut params: serde_json::Value,
    allowlist: &[String],
    denylist: &[String],
) -> serde_json::Value {
    let Some(map) = params.as_object_mut() else {
        return params;
    };
    if !allowlist.is_empty() {
        map.insert("toolAllowlist".to_string(), serde_json::json!(allowlist));
    }
    if !denylist.is_empty() {
        map.insert("toolDenylist".to_string(), serde_json::json!(denylist));
    }
    params
}

/// Build the `mcpServers[]` array for `session/create`/`resume` from the
/// project + user MCP configs. The kernel itself NEVER reads project
/// `.mcp.json` (bundle-verified 2026-07-07: it only appears in plugin
/// loading), so streaming sessions only get MCP servers the client passes
/// here — schema `$xe`, strict union pinned from the kernel bundle:
/// stdio `{name, command, args, env:[{name,value}], timeoutMs?}` (NO type
/// key), remote `{name, type:"http"|"sse", url, headers:[{name,value}],
/// timeoutMs?}`. Disabled servers are skipped; on a name collision the
/// project entry wins. Returns None when nothing survives, so the params
/// stay byte-identical to the pre-MCP shape.
pub fn mcp_servers_param(project: &McpConfig, user: &McpConfig) -> Option<serde_json::Value> {
    let mut merged: BTreeMap<&String, &McpServer> = BTreeMap::new();
    for (name, server) in user.servers.iter().chain(project.servers.iter()) {
        merged.insert(name, server); // later (project) insert wins
    }
    let kv_array = |map: &BTreeMap<String, String>| -> serde_json::Value {
        serde_json::Value::Array(
            map.iter()
                .map(|(name, value)| serde_json::json!({ "name": name, "value": value }))
                .collect(),
        )
    };
    let servers: Vec<serde_json::Value> = merged
        .into_iter()
        .filter(|(_, server)| !server.disabled)
        .filter_map(|(name, server)| {
            if let Some(url) = &server.url {
                // Remote shape; anything that isn't exactly http/sse is
                // normalized to http (the kernel enum allows only those two).
                let transport = match server.transport_label() {
                    "sse" => "sse",
                    _ => "http",
                };
                Some(serde_json::json!({
                    "name": name,
                    "type": transport,
                    "url": url,
                    "headers": kv_array(&server.headers),
                }))
            } else if !server.command.is_empty() {
                Some(serde_json::json!({
                    "name": name,
                    "command": server.command,
                    "args": server.args,
                    "env": kv_array(&server.env),
                }))
            } else {
                None
            }
        })
        .collect();
    if servers.is_empty() {
        None
    } else {
        Some(serde_json::Value::Array(servers))
    }
}

/// Build the `runtimeModel` object the kernel needs to revive a resumed
/// session, from the kernel's own `~/.zcode/cli/config.json` (the same file
/// session/create seeds a fresh session from). Shape pinned live 2026-07-07
/// against the kernel's strict zod schema (`p_` in the bundle):
/// `{revision, generatedAt, model:{providerId,modelId},
///   provider:{providerId, kind, label?, source, baseURL?,
///             apiKey:{source:"inline", value}?, models:[{modelId,label?}…]}}`.
/// Returns None when the config is missing or not in the known layout — the
/// caller resumes without it and relies on the create-fallback path.
pub fn build_runtime_model(config_json: &str, generated_at: u64) -> Option<serde_json::Value> {
    let config: serde_json::Value = serde_json::from_str(config_json).ok()?;
    let main = config
        .pointer("/model/main")
        .and_then(|v| v.as_str())
        .or_else(|| config.get("model").and_then(|v| v.as_str()))?;
    let (provider_id, model_id) = main.split_once('/')?;
    let provider = config.pointer(&format!("/provider/{provider_id}"))?;
    let kind = provider.get("kind")?.as_str()?;
    let models: Vec<serde_json::Value> = provider
        .get("models")?
        .as_object()?
        .iter()
        .map(|(id, m)| {
            serde_json::json!({
                "modelId": id,
                "label": m.get("name").and_then(|v| v.as_str()).unwrap_or(id),
            })
        })
        .collect();
    if models.is_empty() {
        return None;
    }
    let mut provider_obj = serde_json::json!({
        "providerId": provider_id,
        "kind": kind,
        "label": provider.get("name").and_then(|v| v.as_str()).unwrap_or(provider_id),
        "source": "user",
        "models": models,
    });
    if let Some(base_url) = provider
        .pointer("/options/baseURL")
        .and_then(|v| v.as_str())
    {
        provider_obj["baseURL"] = serde_json::json!(base_url);
    }
    if let Some(api_key) = provider.pointer("/options/apiKey").and_then(|v| v.as_str()) {
        // The kernel's credential union; inline carries the key verbatim
        // (same trust domain: the kernel owns config.json to begin with).
        provider_obj["apiKey"] = serde_json::json!({ "source": "inline", "value": api_key });
    }
    Some(serde_json::json!({
        "revision": "zcode-tui-resume",
        "generatedAt": generated_at,
        "model": { "providerId": provider_id, "modelId": model_id },
        "provider": provider_obj,
    }))
}

/// `session/usage` — per-session token breakdown.
pub fn app_usage_params(session_id: &str) -> serde_json::Value {
    serde_json::json!({ "sessionId": session_id })
}

/// `session/read` — a bounded parent-session snapshot whose projection owns
/// the authoritative `contextUsed/contextWindow` pair.
pub fn app_session_read_params(session_id: &str) -> serde_json::Value {
    serde_json::json!({ "sessionId": session_id, "messageLimit": 1 })
}

/// `session/subagents` — authoritative snapshot of child agents and shell
/// work running in the session. Older kernels may reject this method; callers
/// should treat that as an optional capability rather than a session failure.
pub fn app_subagents_params(session_id: &str) -> serde_json::Value {
    serde_json::json!({ "sessionId": session_id })
}

/// `session/cancelBackgroundTask` accepts only the parent session id and the
/// exact kernel task id. Its strict schema rejects display/child/agent ids.
pub fn app_cancel_background_task_params(session_id: &str, task_id: &str) -> serde_json::Value {
    serde_json::json!({ "sessionId": session_id, "taskId": task_id })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CancelBackgroundTaskOutcome {
    pub task_id: String,
    pub cancelled: bool,
    pub status: Option<String>,
    pub reason: Option<String>,
}

pub fn parse_cancel_background_task_result(
    result: &serde_json::Value,
) -> Option<CancelBackgroundTaskOutcome> {
    Some(CancelBackgroundTaskOutcome {
        task_id: result.get("taskId")?.as_str()?.to_string(),
        cancelled: result
            .get("cancelled")
            .and_then(serde_json::Value::as_bool)?,
        status: result
            .get("status")
            .and_then(|value| value.as_str())
            .map(str::to_string),
        reason: result
            .get("reason")
            .and_then(|value| value.as_str())
            .map(str::to_string),
    })
}

/// One normalized row from either `session/subagents` or the V4 state plane.
/// Identifiers deliberately remain separate: kernels use each one for a
/// different control/correlation domain and they are not interchangeable.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AgentSnapshot {
    pub kind: String,
    pub task_id: Option<String>,
    pub child_session_id: Option<String>,
    pub agent_id: Option<String>,
    pub tool_call_id: Option<String>,
    pub title: Option<String>,
    pub summary: Option<String>,
    pub status: Option<String>,
    pub command: Option<String>,
    pub output_tail: Option<String>,
    pub pid: Option<u64>,
    pub cancellable: Option<bool>,
    pub revision: Option<u64>,
}

fn string_alias(value: &serde_json::Value, keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|key| value.get(*key).and_then(|v| v.as_str()))
        .map(str::to_string)
}

fn parse_agent_snapshot(value: &serde_json::Value, kind: &str) -> Option<AgentSnapshot> {
    let snapshot = AgentSnapshot {
        kind: kind.to_string(),
        task_id: string_alias(value, &["taskId", "task_id"]),
        child_session_id: string_alias(value, &["childSessionId", "child_session_id", "sessionId"]),
        agent_id: string_alias(value, &["agentId", "agent_id"]),
        tool_call_id: string_alias(value, &["toolCallId", "tool_call_id"]),
        title: string_alias(value, &["title", "name", "toolName"]),
        summary: string_alias(value, &["summary", "description", "message"]),
        status: string_alias(value, &["status", "state"]),
        command: string_alias(value, &["command"]),
        output_tail: string_alias(value, &["outputTail", "lastOutput", "output"]),
        pid: value.get("pid").and_then(serde_json::Value::as_u64),
        cancellable: value
            .get("cancellable")
            .or_else(|| value.get("canCancel"))
            .and_then(serde_json::Value::as_bool),
        revision: value
            .get("revision")
            .or_else(|| value.get("updatedRevision"))
            .and_then(serde_json::Value::as_u64),
    };
    (snapshot.task_id.is_some()
        || snapshot.child_session_id.is_some()
        || snapshot.agent_id.is_some()
        || snapshot.tool_call_id.is_some())
    .then_some(snapshot)
}

fn append_agent_array(
    output: &mut Vec<AgentSnapshot>,
    value: Option<&serde_json::Value>,
    kind: &str,
) {
    let Some(rows) = value.and_then(|value| value.as_array()) else {
        return;
    };
    output.extend(
        rows.iter()
            .filter_map(|row| parse_agent_snapshot(row, kind)),
    );
}

fn append_agent_container(output: &mut Vec<AgentSnapshot>, value: &serde_json::Value) {
    append_agent_array(output, value.get("subagents"), "subagent");
    append_agent_array(
        output,
        value
            .get("backgroundWorks")
            .or_else(|| value.get("background_works")),
        "background",
    );
}

fn append_official_subagents(output: &mut Vec<AgentSnapshot>, value: &serde_json::Value) {
    let revision = value.get("revision").and_then(serde_json::Value::as_u64);
    let start = output.len();
    append_agent_array(output, value.get("running"), "subagent");
    append_agent_array(output, value.pointer("/ended/items"), "subagent");
    for snapshot in &mut output[start..] {
        snapshot.revision = snapshot.revision.or(revision);
    }
    if let Some(ids) = value.get("childSessionIds").and_then(|ids| ids.as_array()) {
        for id in ids.iter().filter_map(|id| id.as_str()) {
            if !output
                .iter()
                .any(|snapshot| snapshot.child_session_id.as_deref() == Some(id))
            {
                output.push(AgentSnapshot {
                    kind: "subagent".to_string(),
                    child_session_id: Some(id.to_string()),
                    revision,
                    ..Default::default()
                });
            }
        }
    }
}

/// Normalize an authoritative `session/subagents` response. Both the direct
/// result and the observed `{session:{...}}` envelope are accepted.
pub fn parse_subagents_result(result: &serde_json::Value) -> Vec<AgentSnapshot> {
    let mut output = Vec::new();
    append_agent_container(&mut output, result);
    append_official_subagents(&mut output, result);
    if let Some(session) = result.get("session") {
        append_agent_container(&mut output, session);
        append_official_subagents(&mut output, session);
    }
    output
}

/// Extract agent/background-work state carried by a V4 snapshot or delta.
pub fn parse_v4_agent_snapshots(params: &serde_json::Value) -> Vec<AgentSnapshot> {
    let mut output = Vec::new();
    let Some(payload) = params.pointer("/frame/payload") else {
        return output;
    };
    match payload.get("kind").and_then(|value| value.as_str()) {
        Some("snapshot") => {
            if let Some(snapshot) = payload.get("snapshot") {
                append_agent_container(&mut output, snapshot);
            }
        }
        Some("deltas") => {
            if let Some(deltas) = payload.get("deltas").and_then(|value| value.as_array()) {
                for delta in deltas {
                    if let Some(patch) = delta.get("patch") {
                        append_agent_container(&mut output, patch);
                    }
                    if let Some(row) = delta.get("subagent") {
                        if let Some(snapshot) = parse_agent_snapshot(row, "subagent") {
                            output.push(snapshot);
                        }
                    }
                    if let Some(row) = delta
                        .get("backgroundWork")
                        .or_else(|| delta.get("background_work"))
                    {
                        if let Some(snapshot) = parse_agent_snapshot(row, "background") {
                            output.push(snapshot);
                        }
                    }
                }
            }
        }
        _ => {}
    }
    output
}

/// `usage/stats` — period aggregate; kernel zod enum pins range to 7d|30d.
pub fn usage_stats_params(range: &str) -> serde_json::Value {
    serde_json::json!({ "range": range })
}

/// `session/close` — release a session the TUI is discarding (/new, clean
/// exit). Params pinned live 2026-07-07: `{sessionId}` strict (empty params
/// ZodError names sessionId), result `{}`. Fire-and-forget, best-effort.
pub fn app_close_params(session_id: &str) -> serde_json::Value {
    serde_json::json!({ "sessionId": session_id })
}

/// One captured `checkpoint.created` event — a rewind target. The kernel
/// emits one per gated tool write; the snapshot is the workspace state
/// **before** that write ran (pinned live 2026-07-07: rewinding to the first
/// checkpoint of a fresh file DELETES it).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckpointEntry {
    pub id: String,
    /// `fileCount` — files captured by the snapshot.
    pub files: u64,
    /// `targetMessageId` — the turn's user message. The conversation-scope
    /// leg MUST target this via `{kind:"message"}`: pinned live 2026-07-09,
    /// `session/rewind` COERCES checkpoint-kind targets to a workspace (file)
    /// rewind no matter which scope was requested, while message-kind targets
    /// honor scope:"conversation" and leave files alone.
    pub message_id: Option<String>,
}

/// Short display form of a checkpoint id ("checkpoint_90c0d5df-…" → "90c0d5df").
pub fn checkpoint_short_id(id: &str) -> String {
    id.trim_start_matches("checkpoint_")
        .chars()
        .take(8)
        .collect()
}

/// A rewind target — the discriminated union of `session/rewind` and the
/// file-rewind pair. The UI picks checkpoint forms; conversation legs are
/// translated to `Message` (see `conversation_target`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RewindTarget {
    /// `{kind:"latestCheckpoint"}` — kernel-tracked most recent checkpoint.
    LatestCheckpoint,
    /// `{kind:"checkpoint", checkpointId}`.
    Checkpoint(String),
    /// `{kind:"message", messageId}` — the only target kind whose
    /// scope:"conversation" is honored by session/rewind (pinned live
    /// 2026-07-09; checkpoint kinds get coerced to a forced file rewind).
    Message(String),
    /// `{kind:"turn", turnIndex}` — conversation rewind to before that turn.
    Turn(u64),
    /// ZCode 3.5.3 V4 stable row target `{rowId, entityId}`.
    V4Row { row_id: u64, entity_id: String },
}

impl RewindTarget {
    pub fn to_json(&self) -> serde_json::Value {
        match self {
            Self::LatestCheckpoint => serde_json::json!({ "kind": "latestCheckpoint" }),
            Self::Checkpoint(id) => {
                serde_json::json!({ "kind": "checkpoint", "checkpointId": id })
            }
            Self::Message(id) => serde_json::json!({ "kind": "message", "messageId": id }),
            Self::Turn(index) => serde_json::json!({ "kind": "turn", "turnIndex": index }),
            Self::V4Row { row_id, entity_id } => v4_rewind_target(*row_id, entity_id),
        }
    }

    /// Picker/status label, e.g. "latest checkpoint" or "checkpoint 90c0d5df".
    pub fn label(&self) -> String {
        match self {
            Self::LatestCheckpoint => "latest checkpoint".to_string(),
            Self::Checkpoint(id) => format!("checkpoint {}", checkpoint_short_id(id)),
            Self::Message(id) => format!("message {}", &id[..id.len().min(16)]),
            Self::Turn(index) => format!("turn {index}"),
            Self::V4Row { row_id, .. } => format!("turn row {row_id}"),
        }
    }

    pub fn is_v4(&self) -> bool {
        matches!(self, Self::V4Row { .. })
    }
}

/// Translate a picker target (checkpoint form) into the message-kind target
/// its conversation-scope leg must use. None when the checkpoint (or its
/// `targetMessageId`) is unknown — the caller refuses the conversation leg
/// instead of sending a checkpoint target that would force-restore files.
pub fn conversation_target(
    picker: &RewindTarget,
    checkpoints: &[CheckpointEntry],
) -> Option<RewindTarget> {
    let entry = match picker {
        RewindTarget::Checkpoint(id) => checkpoints.iter().find(|c| &c.id == id),
        RewindTarget::LatestCheckpoint => checkpoints.last(),
        // Already conversation-shaped.
        RewindTarget::Message(_) | RewindTarget::Turn(_) => return Some(picker.clone()),
        RewindTarget::V4Row { .. } => return None,
    }?;
    entry.message_id.clone().map(RewindTarget::Message)
}

/// `session/previewFileRewind` / `session/applyFileRewind` — both take
/// `{sessionId, target}` (empty-params ZodError names exactly those two).
pub fn app_file_rewind_params(session_id: &str, target: &RewindTarget) -> serde_json::Value {
    serde_json::json!({ "sessionId": session_id, "target": target.to_json() })
}

/// `session/rewind {sessionId, target, scope}`. scope ∈ conversation|
/// workspace|both. The UI only sends scope:"conversation" and only with
/// message-kind targets — BOTH pinned live: session/rewind FORCE-applies
/// file restores over external modifications (2026-07-07, ignores
/// canApply:false), and it COERCES checkpoint-kind targets to a workspace
/// rewind even when scope:"conversation" was requested (2026-07-09,
/// rewind.triggered came back scope:"workspace" and deleted the file).
/// File restores must go through `session/applyFileRewind` instead.
pub fn app_rewind_params(
    session_id: &str,
    target: &RewindTarget,
    scope: &str,
) -> serde_json::Value {
    serde_json::json!({
        "sessionId": session_id,
        "target": target.to_json(),
        "scope": scope,
    })
}

/// One file row of a rewind preview.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RewindFile {
    pub path: String,
    /// safeFiles: `action` ("restore"); unsafeFiles: `reason`
    /// ("external_modified", …).
    pub note: String,
    /// Joined `toolNames`.
    pub tools: String,
}

/// Parsed `session/previewFileRewind` result (shape pinned live 2026-07-07:
/// `{canApply, safeFiles[{action,operationCount,path,toolNames}],
/// unsafeFiles[{path,reason,expectedHash,currentHash,…}], ignoredFiles,…}`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RewindPreview {
    pub can_apply: bool,
    pub safe: Vec<RewindFile>,
    pub unsafe_files: Vec<RewindFile>,
    pub ignored: usize,
}

fn rewind_files(result: &serde_json::Value, key: &str, note_key: &str) -> Vec<RewindFile> {
    result
        .get(key)
        .and_then(|v| v.as_array())
        .map(|files| {
            files
                .iter()
                .filter_map(|file| {
                    let path = file.get("path")?.as_str()?.to_string();
                    let tools = file
                        .get("toolNames")
                        .and_then(|v| v.as_array())
                        .map(|names| {
                            names
                                .iter()
                                .filter_map(|n| n.as_str())
                                .collect::<Vec<_>>()
                                .join(", ")
                        })
                        .unwrap_or_default();
                    Some(RewindFile {
                        path,
                        note: str_at(file, note_key),
                        tools,
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Parse a previewFileRewind result. Missing `canApply` -> None (not a
/// preview shape); empty file lists are valid (nothing to restore).
pub fn parse_rewind_preview(result: &serde_json::Value) -> Option<RewindPreview> {
    let can_apply = result.get("canApply")?.as_bool()?;
    Some(RewindPreview {
        can_apply,
        safe: rewind_files(result, "safeFiles", "action"),
        unsafe_files: rewind_files(result, "unsafeFiles", "reason"),
        ignored: result
            .get("ignoredFiles")
            .and_then(|v| v.as_array())
            .map_or(0, Vec::len),
    })
}

/// Outcome of `session/applyFileRewind` (shape pinned live 2026-07-07:
/// `{applied: bool, preview: {…}, response: string}`; refusals keep
/// `applied:false` with the unsafe files in the embedded preview).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileRewindOutcome {
    pub applied: bool,
    pub response: String,
    /// Unsafe rows of the embedded preview ("reason path"), for the refusal
    /// report.
    pub unsafe_files: Vec<RewindFile>,
}

pub fn parse_apply_file_rewind(result: &serde_json::Value) -> FileRewindOutcome {
    FileRewindOutcome {
        applied: result
            .get("applied")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false),
        response: str_at(result, "response"),
        unsafe_files: result
            .get("preview")
            .map(|preview| rewind_files(preview, "unsafeFiles", "reason"))
            .unwrap_or_default(),
    }
}

/// Judge a `session/rewind` outcome. NEVER trust the envelope: a rewind to a
/// nonexistent checkpoint returns a SUCCESS envelope whose `response` reads
/// "Checkpoint … was not found." (pinned live 2026-07-07). The only reliable
/// signal is the `rewind.triggered` session event: `strategy:"active_chain"`
/// = applied, `strategy:"unavailable"` (+ `reason`, e.g.
/// "target_checkpoint_not_found") = nothing happened. Returns the failure
/// text, or None on success.
pub fn rewind_failure(
    strategy: Option<&str>,
    reason: Option<&str>,
    response: &str,
) -> Option<String> {
    match strategy {
        Some("unavailable") => Some(if response.is_empty() {
            format!("rewind unavailable: {}", reason.unwrap_or("unknown reason"))
        } else {
            response.to_string()
        }),
        Some(_) => None,
        // No rewind.triggered observed at all — do not claim success.
        None => Some(format!(
            "no rewind.triggered event observed (kernel said: {})",
            if response.is_empty() {
                "nothing"
            } else {
                response
            }
        )),
    }
}

/// One replayable history message from a `session/resume` result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplayMessage {
    /// "user" | "assistant".
    pub role: String,
    /// Concatenated text parts, truncated to the preview cap.
    pub preview: String,
}

/// Extract the LAST up-to-`limit` renderable messages from a resume result's
/// `messages[]` (shape pinned live 2026-07-07: `{info:{role,…},
/// parts:[{type:"text", text}|{type:"reasoning"|"file"|"step-*",…}…]}`).
/// Only user/assistant roles count; only `type:"text"` parts contribute;
/// empty texts are skipped; previews are char-truncated to `cap` with "…".
pub fn parse_resume_messages(
    result: &serde_json::Value,
    limit: usize,
    cap: usize,
) -> Vec<ReplayMessage> {
    let Some(messages) = result.get("messages").and_then(|m| m.as_array()) else {
        return Vec::new();
    };
    let mut replay: Vec<ReplayMessage> = messages
        .iter()
        .filter_map(|message| {
            let role = message.pointer("/info/role")?.as_str()?;
            if role != "user" && role != "assistant" {
                return None;
            }
            let text = message
                .get("parts")?
                .as_array()?
                .iter()
                .filter(|part| part.get("type").and_then(|t| t.as_str()) == Some("text"))
                .filter_map(|part| part.get("text").and_then(|t| t.as_str()))
                .collect::<Vec<_>>()
                .join("\n");
            let trimmed = text.trim();
            if trimmed.is_empty() {
                return None;
            }
            let mut preview: String = trimmed.chars().take(cap).collect();
            if trimmed.chars().count() > cap {
                preview.push('…');
            }
            Some(ReplayMessage {
                role: role.to_string(),
                preview,
            })
        })
        .collect();
    if replay.len() > limit {
        replay.drain(..replay.len() - limit);
    }
    replay
}

/// Standard base64 (RFC 4648, with padding) — hand-rolled to keep the
/// dependency tree flat; only used for the OSC52 clipboard payload.
pub fn base64_encode(input: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(input.len().div_ceil(3) * 4);
    for chunk in input.chunks(3) {
        let b = [
            chunk[0],
            chunk.get(1).copied().unwrap_or(0),
            chunk.get(2).copied().unwrap_or(0),
        ];
        let n = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | b[2] as u32;
        out.push(TABLE[(n >> 18) as usize & 63] as char);
        out.push(TABLE[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            TABLE[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            TABLE[n as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

/// OSC52 clipboard-set sequence (`ESC ] 52 ; c ; <base64> BEL`) for `text`,
/// or None when text is empty. `max_b64` caps the encoded payload (~100KB by
/// convention — terminals truncate or reject oversized sequences); the SOURCE
/// text is truncated on a char boundary first so the base64 is always valid.
pub fn osc52_copy_sequence(text: &str, max_b64: usize) -> Option<String> {
    if text.is_empty() {
        return None;
    }
    let max_src = max_b64 / 4 * 3;
    let mut source = text;
    if source.len() > max_src {
        // Truncate to the last char boundary at or below the byte cap.
        let mut cut = max_src;
        while cut > 0 && !source.is_char_boundary(cut) {
            cut -= 1;
        }
        source = &source[..cut];
        if source.is_empty() {
            return None;
        }
    }
    Some(format!(
        "\x1b]52;c;{}\x07",
        base64_encode(source.as_bytes())
    ))
}

/// Parse a `session/list` result (`sessions[]{sessionId,title,workspace,
/// updatedAt,status,…}`) into picker rows: current-`cwd` sessions first,
/// then by recency — mirroring the db-backed `list_recent_sessions` order.
/// Sessions still `running` get a marker suffix so the picker can show it.
pub fn parse_session_list(result: &serde_json::Value, cwd: &str) -> Vec<SessionRow> {
    let Some(sessions) = result.get("sessions").and_then(|v| v.as_array()) else {
        return Vec::new();
    };
    let mut rows: Vec<(bool, SessionRow)> = sessions
        .iter()
        .filter_map(|s| {
            let id = s.get("sessionId")?.as_str()?.to_string();
            let directory = s
                .pointer("/workspace/workspacePath")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string();
            let mut title = s
                .get("title")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string();
            if title.is_empty() {
                title = directory.rsplit('/').next().unwrap_or_default().to_string();
            }
            if s.get("status").and_then(|v| v.as_str()) == Some("running") {
                title.push_str("  · running");
            }
            let time_updated = s
                .get("updatedAt")
                .and_then(serde_json::Value::as_i64)
                .unwrap_or(0);
            Some((
                directory == cwd,
                SessionRow {
                    id,
                    title,
                    directory,
                    time_updated,
                },
            ))
        })
        .collect();
    rows.sort_by(|a, b| b.0.cmp(&a.0).then(b.1.time_updated.cmp(&a.1.time_updated)));
    rows.into_iter().map(|(_, row)| row).collect()
}

/// Outcome of a `session/steer` request. The SUCCESS envelope's result is a
/// discriminated union (kernel `FKr`): `{kind:"queued",…}` means the input
/// entered the running turn; `{kind:"rejected", reason}` means it did NOT —
/// treating an ok envelope as success silently loses rejected input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SteerOutcome {
    Queued,
    Rejected(String),
    /// Unrecognized result shape (older/newer kernel): assume queued rather
    /// than double-submitting the input.
    Unknown,
}

pub fn parse_steer_result(result: &serde_json::Value) -> SteerOutcome {
    match result.get("kind").and_then(|v| v.as_str()) {
        Some("queued") => SteerOutcome::Queued,
        Some("rejected") => SteerOutcome::Rejected(
            result
                .get("reason")
                .and_then(|v| v.as_str())
                .unwrap_or("rejected")
                .to_string(),
        ),
        _ => SteerOutcome::Unknown,
    }
}

/// A kernel-reported slash command (`session/create`/`resume` result's
/// `slashCommands[]`), merged into `/` completion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KernelCommand {
    pub name: String,
    pub description: String,
    pub input_hint: String,
}

pub fn parse_kernel_slash_commands(result: &serde_json::Value) -> Vec<KernelCommand> {
    result
        .get("slashCommands")
        .and_then(|v| v.as_array())
        .map(|list| {
            list.iter()
                .filter_map(|c| {
                    let name = c.get("name")?.as_str()?.to_string();
                    Some(KernelCommand {
                        name,
                        description: c
                            .get("description")
                            .and_then(|v| v.as_str())
                            .unwrap_or_default()
                            .to_string(),
                        input_hint: c
                            .get("inputHint")
                            .and_then(|v| v.as_str())
                            .unwrap_or_default()
                            .to_string(),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// An owned suggestion row: the local catalog merged with kernel-reported
/// commands (local implementations win on name collisions).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlashEntry {
    pub command: String,
    pub summary: String,
    pub route: String,
}

/// `/` completion over local + kernel commands. Local entries keep their
/// catalog order and priority; kernel commands come after, deduped by base
/// name (`/goal …` vs local `/goal`), showing their inputHint as the command
/// when it adds information.
pub fn slash_suggestions_merged(
    input: &str,
    limit: usize,
    kernel: &[KernelCommand],
) -> Vec<SlashEntry> {
    let query = input.trim();
    if query.is_empty() || !query.starts_with('/') || limit == 0 {
        return Vec::new();
    }
    let bare = query.trim_start_matches('/');
    let local_names: std::collections::HashSet<&str> = command_catalog()
        .iter()
        .filter_map(|item| item.command.strip_prefix('/'))
        .map(|rest| rest.split_whitespace().next().unwrap_or(rest))
        .collect();
    let mut catalog: Vec<SlashEntry> = command_catalog()
        .iter()
        .map(|item| SlashEntry {
            command: item.command.to_string(),
            summary: item.summary.to_string(),
            route: item.route.to_string(),
        })
        .collect();
    for command in kernel {
        if local_names.contains(command.name.as_str()) {
            continue;
        }
        let display = if command.input_hint.starts_with('/') {
            command.input_hint.clone()
        } else {
            format!("/{}", command.name)
        };
        catalog.push(SlashEntry {
            command: display,
            summary: command.description.clone(),
            route: "zcode".to_string(),
        });
    }
    let mut scored: Vec<(u8, usize, SlashEntry)> = Vec::new();
    for (index, item) in catalog.into_iter().enumerate() {
        let rank = if item.command.starts_with(query) {
            0
        } else if !bare.is_empty() && item.command.contains(bare) {
            1
        } else if is_subsequence(query, &item.command) {
            2
        } else {
            continue;
        };
        scored.push((rank, index, item));
    }
    scored.sort_by_key(|a| (a.0, a.1));
    scored
        .into_iter()
        .take(limit)
        .map(|(_, _, item)| item)
        .collect()
}

/// One kernel TODO item (create/resume result's `todos[]` or a state push).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TodoItem {
    pub text: String,
    pub done: bool,
}

/// Extract todos from a create/resume result or a `state.updated` patch.
/// Tolerates both `{content|text|title, status|completed}` item shapes; an
/// absent list -> empty (caller keeps its previous list only on pushes that
/// carry no `todos` key at all — an empty array is an explicit clear).
pub fn parse_todos(value: &serde_json::Value) -> Option<Vec<TodoItem>> {
    let list = value
        .get("todos")
        .or_else(|| value.pointer("/patch/todos"))?
        .as_array()?;
    Some(
        list.iter()
            .filter_map(|t| {
                let text = t
                    .get("content")
                    .or_else(|| t.get("text"))
                    .or_else(|| t.get("title"))?
                    .as_str()?
                    .to_string();
                let done = t
                    .get("status")
                    .and_then(|v| v.as_str())
                    .map(|s| s.eq_ignore_ascii_case("completed") || s.eq_ignore_ascii_case("done"))
                    .or_else(|| t.get("completed").and_then(serde_json::Value::as_bool))
                    .unwrap_or(false);
                Some(TodoItem { text, done })
            })
            .collect(),
    )
}

/// One selectable model from the state push's `model.available[]`.
#[derive(Debug, Clone, PartialEq)]
pub struct ModelChoice {
    pub label: String,
    pub provider: String,
    pub context_window: Option<u64>,
    /// `available[].ref`, echoed back verbatim in `session/setModel`.
    pub reference: serde_json::Value,
}

/// The session control surface carried by a `state.updated` patch (all fields
/// optional — pushes are partial; the consumer merges non-empty fields).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SessionControls {
    pub mode: Option<String>,
    pub models: Vec<ModelChoice>,
    /// Current model's `providerId`.
    pub model_provider: Option<String>,
    /// Current model's `modelId`.
    pub model_current: Option<String>,
    pub thought_levels: Vec<String>,
    pub thought_current: Option<String>,
}

fn controls_from_settings(settings: &serde_json::Value) -> Option<SessionControls> {
    let controls = SessionControls {
        mode: settings
            .pointer("/mode/current")
            .and_then(|v| v.as_str())
            .map(str::to_string),
        models: settings
            .pointer("/model/available")
            .and_then(|v| v.as_array())
            .map(|list| {
                list.iter()
                    .filter_map(|m| {
                        Some(ModelChoice {
                            label: m.get("label")?.as_str()?.to_string(),
                            provider: m
                                .get("providerLabel")
                                .and_then(|v| v.as_str())
                                .unwrap_or_default()
                                .to_string(),
                            context_window: m
                                .get("contextWindow")
                                .and_then(serde_json::Value::as_u64),
                            reference: m.get("ref")?.clone(),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default(),
        model_provider: settings
            .pointer("/model/current/providerId")
            .and_then(|v| v.as_str())
            .map(str::to_string),
        model_current: settings
            .pointer("/model/current/modelId")
            .and_then(|v| v.as_str())
            .map(str::to_string),
        thought_levels: settings
            .pointer("/thoughtLevel/available")
            .and_then(|v| v.as_array())
            .map(|list| {
                list.iter()
                    .filter_map(|l| l.get("value").and_then(|v| v.as_str()).map(str::to_string))
                    .collect()
            })
            .unwrap_or_default(),
        thought_current: settings
            .pointer("/thoughtLevel/current")
            .and_then(|v| v.as_str())
            .map(str::to_string),
    };
    let empty = controls.mode.is_none()
        && controls.models.is_empty()
        && controls.model_provider.is_none()
        && controls.model_current.is_none()
        && controls.thought_levels.is_empty()
        && controls.thought_current.is_none();
    if empty {
        None
    } else {
        Some(controls)
    }
}

/// `workspace/readState` params for the ZCode app-server.
pub fn app_workspace_read_params(workspace_path: &str) -> serde_json::Value {
    serde_json::json!({
        "workspace": {
            "workspaceKey": workspace_path,
            "workspacePath": workspace_path,
        }
    })
}

/// Extract the active provider and its models from `workspace/readState`.
///
/// ZCode owns provider authentication and catalog discovery. The response
/// contains model metadata only, so callers never need provider credentials.
pub fn app_workspace_model_controls(
    result: &serde_json::Value,
) -> Option<(String, SessionControls)> {
    let settings = result.get("settings")?;
    let provider_id = settings
        .pointer("/model/current/providerId")?
        .as_str()?
        .to_string();
    let mut controls = controls_from_settings(settings)?;
    controls.models.retain(|model| {
        model
            .reference
            .get("providerId")
            .and_then(serde_json::Value::as_str)
            == Some(provider_id.as_str())
    });
    if controls.models.is_empty() {
        return None;
    }
    Some((provider_id, controls))
}

/// Extract control state from a `session/create` or `session/resume` result.
///
/// The top-level `settings` contains the complete model catalog. The nested
/// snapshot can contain only the current model, so it is fallback-only.
pub fn app_session_controls(result: &serde_json::Value) -> Option<SessionControls> {
    result
        .get("settings")
        .and_then(controls_from_settings)
        .or_else(|| {
            result
                .pointer("/snapshot/settings")
                .and_then(controls_from_settings)
        })
}

/// Extract whatever control-surface state a `state.updated` push carries
/// (`reason:"mode_changed"` carries the full set; others may carry parts).
/// None when the patch has none of the control keys.
pub fn app_state_controls(params: &serde_json::Value) -> Option<SessionControls> {
    params.get("patch").and_then(controls_from_settings)
}

/// A decoded inbound line: a response to one of our requests, a session
/// event (the token stream), a session-level state update, a server→client
/// request (the kernel asking *us* something), or ignorable.
// The Event variant (AppServerEvent) is the largest, but every message is
// decoded and matched immediately (never stored in bulk), so boxing it would
// only add a heap allocation per streamed event on the hot path.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, PartialEq)]
pub enum AppServerMessage {
    /// Response to request `id`; `error` set means the request failed.
    Response {
        id: u64,
        result: Option<serde_json::Value>,
        error: Option<String>,
    },
    /// `session/event` payload — the streaming turn events.
    Event(AppServerEvent),
    /// `state.updated` — session status/mode/model/context watermark.
    StateUpdated(serde_json::Value),
    /// ZCode 3.5.3 V4 conversation snapshot/delta transport frame.
    V4Frame(serde_json::Value),
    /// Server→client request: carries `method` AND an envelope `id` we must
    /// echo back in the reply. The kernel uses STRING ids here (`"server-1"`,
    /// `"server-2"`, …) so the id is kept as raw JSON and returned verbatim
    /// (`interaction/requestUserInput` is the permission-approval channel).
    ServerRequest {
        id: serde_json::Value,
        method: String,
        params: serde_json::Value,
    },
    /// A recognized-but-uninteresting line; skipped without failing.
    Other,
}

/// One `session/event` payload. `kind` drives dispatch; the rest are set only
/// for the kinds that carry them (tool events, `result`). Defaulted so tests
/// and non-tool events can build it with just `kind`/`delta`/`done`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AppServerEvent {
    pub kind: String,
    pub delta: String,
    pub done: bool,
    /// `toolName` (tool_input_start / tool_call / started).
    pub tool_name: Option<String>,
    /// `toolCallId` — correlates a tool across its start/input/result events.
    pub tool_call_id: Option<String>,
    /// `result.result.content` — the tool's output text (on `kind=result`).
    pub output: Option<String>,
    /// `result.result.success` — tool succeeded (on `kind=result`).
    pub success: Option<bool>,
    /// `result.duration` — tool wall time in ms (on `kind=result`).
    pub duration_ms: Option<u64>,
    /// `payload.fileCount` — files captured by a `checkpoint.created`
    /// session-level event (surfaced via `params.type` passthrough).
    pub file_count: Option<u64>,
    /// `payload.checkpointId` (`checkpoint.created`) — the rewind target id;
    /// captured per session for the /rewind picker.
    pub checkpoint_id: Option<String>,
    /// `payload.targetMessageId` (falling back to `payload.messageId`) of a
    /// `checkpoint.created` — the turn's user message, needed as the
    /// `{kind:"message"}` target of the conversation-scope leg.
    pub target_message_id: Option<String>,
    /// `payload.strategy` (`rewind.triggered`) — "active_chain" on a real
    /// rewind, "unavailable" when the kernel could NOT rewind. Pinned live
    /// 2026-07-07: a failed rewind still returns a SUCCESS envelope, so this
    /// event is the only trustworthy outcome signal.
    pub strategy: Option<String>,
    /// `payload.reason` (`rewind.triggered`) — e.g. "target_in_active_chain",
    /// "target_checkpoint_not_found".
    pub reason: Option<String>,
    /// `payload.taskId` — `background_task_*` events used by ZCode 3.3.4
    /// subagent/bash backgrounding. Decoded so future app-server deliveries are
    /// never silently dropped.
    pub task_id: Option<String>,
    /// Child session id for a subagent. Never conflated with task/tool ids.
    pub child_session_id: Option<String>,
    /// Stable agent identity when supplied by the kernel.
    pub agent_id: Option<String>,
    /// `payload.command` — the backgrounded shell command.
    pub command: Option<String>,
    /// `payload.status` — background task status (running|completed|lost…).
    pub status: Option<String>,
    /// `payload.pid` — background task process id.
    pub pid: Option<u64>,
    pub title: Option<String>,
    pub summary: Option<String>,
    pub cancellable: Option<bool>,
    pub revision: Option<u64>,
}

/// Decode a single inbound protocol line. Unparseable lines -> None (skip).
pub fn decode_app_message(line: &str) -> Option<AppServerMessage> {
    let value: serde_json::Value = serde_json::from_str(line.trim()).ok()?;
    // Server→client request: method AND id together (the kernel expects a
    // reply). Must be checked before the Response branch — its envelope id is
    // a string ("server-N"), and a u64-only path would drop it on the floor
    // (observed: ignored interaction requests hang plan-mode turns until the
    // 600s backstop).
    if let (Some(method), Some(id)) = (
        value.get("method").and_then(|m| m.as_str()),
        value.get("id"),
    ) {
        return Some(AppServerMessage::ServerRequest {
            id: id.clone(),
            method: method.to_string(),
            params: value
                .get("params")
                .cloned()
                .unwrap_or(serde_json::Value::Null),
        });
    }
    // Response: has an `id` and either result or error.
    if let Some(id) = value.get("id").and_then(serde_json::Value::as_u64) {
        let error = value.get("error").map(|err| {
            err.get("message")
                .and_then(|m| m.as_str())
                .unwrap_or("app-server error")
                .to_string()
        });
        let result = value.get("result").cloned();
        return Some(AppServerMessage::Response { id, result, error });
    }
    match value.get("method").and_then(|m| m.as_str()) {
        Some("session/event") => {
            let payload = value.pointer("/params/payload")?;
            let str_field = |key: &str| {
                payload
                    .get(key)
                    .and_then(|v| v.as_str())
                    .map(str::to_string)
            };
            // Streaming payloads (model.streaming) carry their own `kind`;
            // session-level events (checkpoint.created, turn.*, …) do NOT —
            // pass `params.type` through as the kind so they are consumable
            // instead of dropped as unparseable (neither present -> skip).
            let kind = payload
                .get("kind")
                .and_then(|k| k.as_str())
                .or_else(|| value.pointer("/params/type").and_then(|t| t.as_str()))?
                .to_string();
            Some(AppServerMessage::Event(AppServerEvent {
                kind,
                delta: payload
                    .get("delta")
                    .and_then(|d| d.as_str())
                    .unwrap_or_default()
                    .to_string(),
                done: payload
                    .get("done")
                    .and_then(serde_json::Value::as_bool)
                    .unwrap_or(false),
                tool_name: str_field("toolName"),
                tool_call_id: str_field("toolCallId"),
                // `result` events nest the tool output under /result/content.
                output: payload
                    .pointer("/result/content")
                    .and_then(|v| v.as_str())
                    .map(str::to_string),
                success: payload
                    .pointer("/result/success")
                    .and_then(serde_json::Value::as_bool),
                duration_ms: payload.get("duration").and_then(serde_json::Value::as_u64),
                file_count: payload.get("fileCount").and_then(serde_json::Value::as_u64),
                checkpoint_id: str_field("checkpointId"),
                target_message_id: str_field("targetMessageId").or_else(|| str_field("messageId")),
                strategy: str_field("strategy"),
                reason: str_field("reason"),
                task_id: str_field("taskId"),
                child_session_id: str_field("childSessionId"),
                agent_id: str_field("agentId"),
                command: str_field("command"),
                status: str_field("status"),
                pid: payload.get("pid").and_then(serde_json::Value::as_u64),
                title: str_field("title").or_else(|| str_field("name")),
                summary: str_field("summary").or_else(|| str_field("message")),
                cancellable: payload
                    .get("cancellable")
                    .or_else(|| payload.get("canCancel"))
                    .and_then(serde_json::Value::as_bool),
                revision: payload.get("revision").and_then(serde_json::Value::as_u64),
            }))
        }
        Some("state.updated") => Some(AppServerMessage::StateUpdated(
            value
                .get("params")
                .cloned()
                .unwrap_or(serde_json::Value::Null),
        )),
        Some("v4/conversation/frame") => Some(AppServerMessage::V4Frame(
            value
                .get("params")
                .cloned()
                .unwrap_or(serde_json::Value::Null),
        )),
        _ => Some(AppServerMessage::Other),
    }
}

/// The kernel's user-input interaction request (plan approval): questions
/// with options, answered as `{requestId, answers:{header:value}}`.
pub const INTERACTION_METHOD: &str = "interaction/requestUserInput";
/// The kernel's tool-permission request (gated side-effect tools, e.g. Write
/// in build mode): flat options each carrying a ready-made `response` object,
/// answered by echoing the chosen option's `response` VERBATIM (the kernel's
/// result schema is strict — adding any key, even requestId, is rejected).
pub const PERMISSION_METHOD: &str = "interaction/requestPermission";
/// ZCode 0.16.3 asks the client for runtime preferences while materializing a
/// create/resume session. Leaving this unanswered makes session/create time
/// out after 15 seconds and disables app-server streaming.
pub const RUNTIME_PREFERENCES_METHOD: &str = "session/requestRuntimePreferences";

/// Exact 0.16.3 runtime-preferences reply. These are the same compatibility
/// defaults the kernel uses when an older client returns Method not found.
/// The optional integratedTerminalShell is deliberately absent so the kernel
/// selects the host shell normally.
pub fn encode_runtime_preferences_reply(
    envelope_id: &serde_json::Value,
    method: &str,
) -> Option<String> {
    if method != RUNTIME_PREFERENCES_METHOD {
        return None;
    }
    Some(
        serde_json::json!({
            "id": envelope_id,
            "result": {
                "nativeSearchEnhancementsEnabled": true,
                "memoryEnabled": false,
                "askUserQuestionAutoResolutionEnabled": true,
                "modelContextBudgetStrategy": "preflight-v1",
            }
        })
        .to_string(),
    )
}

/// How the reply's `result` must be built — the two interaction methods use
/// incompatible result schemas (both pinned live 2026-07-07).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InteractionReply {
    /// requestUserInput: `{"requestId":…, "answers":{<header>:<value>}}`.
    Answers,
    /// requestPermission: the chosen option's `response` object verbatim.
    Permission,
}

/// One selectable option of an interaction request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InteractionOption {
    pub label: String,
    /// Answer value (requestUserInput) or optionId (requestPermission).
    pub value: String,
    pub description: String,
    /// requestPermission only: the pre-baked reply `result` for this option.
    pub response: Option<serde_json::Value>,
}

/// One question of an interaction request. `header` doubles as the answer key
/// in the reply (`answers: {<header>: <option value>}`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InteractionQuestion {
    pub header: String,
    pub question: String,
    pub options: Vec<InteractionOption>,
}

/// A parsed interaction request (kernel 0.15.0, pinned 2026-07-07): the
/// kernel re-sends the same `request_id` under fresh envelope ids with
/// backoff until answered, so consumers must dedupe on `request_id`.
#[derive(Debug, Clone, PartialEq)]
pub struct InteractionRequest {
    pub request_id: String,
    /// Top line, e.g. "Tool ExitPlanMode requires user interaction" or the
    /// permission reason ("Tool has side effects and requires approval").
    pub prompt: String,
    /// `schema.interaction` (e.g. "plan_approval"), or "permission".
    pub interaction: String,
    pub tool_name: String,
    /// `input.plan` — the plan text under review (plan_approval).
    pub plan: Option<String>,
    pub questions: Vec<InteractionQuestion>,
    pub reply: InteractionReply,
    /// Index of a protocol-level decline option (permission `kind:"deny"`),
    /// answered on Esc. None -> declining falls back to stopping the turn.
    pub deny_index: Option<usize>,
}

fn str_at(value: &serde_json::Value, key: &str) -> String {
    value
        .get(key)
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string()
}

/// Parse a server→client interaction request by method. Requires `requestId`
/// and at least one answerable option — anything less returns None and the
/// caller leaves the request unanswered (the kernel's retry keeps it alive
/// for a future, more capable client).
pub fn parse_interaction_request(
    method: &str,
    params: &serde_json::Value,
) -> Option<InteractionRequest> {
    match method {
        INTERACTION_METHOD => parse_user_input_request(params),
        PERMISSION_METHOD => parse_permission_request(params),
        _ => None,
    }
}

fn parse_user_input_request(params: &serde_json::Value) -> Option<InteractionRequest> {
    let request_id = params.get("requestId")?.as_str()?.to_string();
    let questions: Vec<InteractionQuestion> = params
        .get("questions")?
        .as_array()?
        .iter()
        .filter_map(|q| {
            let options: Vec<InteractionOption> = q
                .get("options")?
                .as_array()?
                .iter()
                .filter_map(|o| {
                    let value = o.get("value")?.as_str()?.to_string();
                    Some(InteractionOption {
                        label: {
                            let label = str_at(o, "label");
                            if label.is_empty() {
                                value.clone()
                            } else {
                                label
                            }
                        },
                        value,
                        description: str_at(o, "description"),
                        response: None,
                    })
                })
                .collect();
            if options.is_empty() {
                return None;
            }
            Some(InteractionQuestion {
                header: str_at(q, "header"),
                question: str_at(q, "question"),
                options,
            })
        })
        .collect();
    if questions.is_empty() {
        return None;
    }
    Some(InteractionRequest {
        request_id,
        prompt: str_at(params, "prompt"),
        interaction: params
            .pointer("/schema/interaction")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string(),
        tool_name: str_at(params, "toolName"),
        plan: params
            .pointer("/input/plan")
            .and_then(|v| v.as_str())
            .map(str::to_string),
        questions,
        reply: InteractionReply::Answers,
        deny_index: None,
    })
}

fn parse_permission_request(params: &serde_json::Value) -> Option<InteractionRequest> {
    let request_id = params.get("requestId")?.as_str()?.to_string();
    let tool_name = str_at(params, "toolName");
    let options: Vec<InteractionOption> = params
        .get("options")?
        .as_array()?
        .iter()
        .filter_map(|o| {
            let option_id = o.get("optionId")?.as_str()?.to_string();
            Some(InteractionOption {
                label: {
                    let name = str_at(o, "name");
                    if name.is_empty() {
                        option_id.clone()
                    } else {
                        name
                    }
                },
                value: option_id,
                description: str_at(o, "description"),
                response: Some(o.get("response")?.clone()),
            })
        })
        .collect();
    if options.is_empty() {
        return None;
    }
    let deny_index = params
        .get("options")
        .and_then(|v| v.as_array())
        .and_then(|list| {
            list.iter()
                .position(|o| o.get("kind").and_then(|k| k.as_str()) == Some("deny"))
        });
    // What the tool wants to do, condensed: "Write  w.txt · hi (risk medium)".
    let summary = params
        .get("input")
        .map(|input| tool_input_summary(&input.to_string()))
        .unwrap_or_default();
    let risk = str_at(params, "riskLevel");
    let mut question = tool_name.clone();
    if !summary.is_empty() {
        question.push_str(&format!("  {summary}"));
    }
    if !risk.is_empty() {
        question.push_str(&format!("  (risk {risk})"));
    }
    Some(InteractionRequest {
        request_id,
        prompt: str_at(params, "reason"),
        interaction: "permission".to_string(),
        tool_name,
        plan: None,
        questions: vec![InteractionQuestion {
            header: String::new(),
            question,
            options,
        }],
        reply: InteractionReply::Permission,
        deny_index,
    })
}

/// Encode the reply for `selected` (an index into the first question's
/// options) as one compact JSON line; the envelope `id` is echoed back
/// verbatim (string or number). Returns None if `selected` is out of bounds
/// or the option lacks its reply payload.
pub fn encode_interaction_reply(
    envelope_id: &serde_json::Value,
    request: &InteractionRequest,
    selected: usize,
) -> Option<String> {
    let result = match request.reply {
        InteractionReply::Answers => {
            // The selection answers the first question; any further questions
            // get their first option (observed payloads carry exactly one).
            let mut answers = serde_json::Map::new();
            for (index, question) in request.questions.iter().enumerate() {
                let pick = if index == 0 { selected } else { 0 };
                let option = question.options.get(pick)?;
                answers.insert(
                    question.header.clone(),
                    serde_json::Value::String(option.value.clone()),
                );
            }
            serde_json::json!({
                "requestId": request.request_id,
                "answers": serde_json::Value::Object(answers),
            })
        }
        // Strict kernel schema: the option's response object and NOTHING else.
        InteractionReply::Permission => request
            .questions
            .first()?
            .options
            .get(selected)?
            .response
            .clone()?,
    };
    Some(serde_json::json!({ "id": envelope_id, "result": result }).to_string())
}

/// A compact one-line summary of a tool's JSON input, for the chip header:
/// `{"file_path":"/a/b/notes.txt"}` -> `notes.txt`. Joins the string values
/// (path-basenamed), collapses whitespace, and caps at ~48 chars. Falls back to
/// the trimmed raw input when it isn't a JSON object.
pub fn tool_input_summary(input: &str) -> String {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    let raw = match serde_json::from_str::<serde_json::Value>(trimmed) {
        Ok(serde_json::Value::Object(map)) => {
            let parts: Vec<&str> = map
                .values()
                .filter_map(|v| v.as_str())
                .map(|s| s.rsplit('/').next().unwrap_or(s))
                .filter(|s| !s.is_empty())
                .collect();
            if parts.is_empty() {
                trimmed.to_string()
            } else {
                parts.join(" ")
            }
        }
        _ => trimmed.to_string(),
    };
    let collapsed = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.chars().count() > 48 {
        let mut out: String = collapsed.chars().take(47).collect();
        out.push('…');
        out
    } else {
        collapsed
    }
}

/// Project a completed internal tool call into a compact transcript entry.
/// Successful mechanical output is summarized; failures retain a bounded
/// diagnostic tail. The full payload remains available in protocol/debug data.
pub fn tool_result_summary(
    name: &str,
    input: &str,
    output: &str,
    success: bool,
    duration_ms: Option<u64>,
) -> String {
    let label = if name.trim().is_empty() {
        "tool"
    } else {
        name.trim()
    };
    let kind = label.to_ascii_lowercase();
    let input = tool_input_summary(input);
    let output = output.trim_end();
    let line_count = output
        .lines()
        .filter(|line| !line.trim().is_empty())
        .count();

    let mut summary = label.to_string();
    if !input.is_empty() {
        summary.push_str(&format!("  {input}"));
    }
    if kind.contains("read") && line_count > 0 {
        summary.push_str(&format!("  · {line_count} lines"));
    } else if (kind.contains("search") || kind.contains("glob") || kind.contains("grep"))
        && line_count > 0
    {
        summary.push_str(&format!("  · {line_count} matches"));
    }
    if let Some(ms) = duration_ms {
        if ms >= 1000 {
            summary.push_str(&format!("  · {:.1}s", ms as f32 / 1000.0));
        } else {
            summary.push_str(&format!("  · {ms}ms"));
        }
    }
    summary.push_str(if success {
        "  · passed"
    } else {
        "  · failed"
    });

    if !success && !output.is_empty() {
        let diagnostics = output
            .lines()
            .filter(|line| !line.trim().is_empty())
            .collect::<Vec<_>>();
        let hidden = diagnostics.len().saturating_sub(4);
        for line in diagnostics.iter().rev().take(4).rev() {
            let clipped = line.chars().take(160).collect::<String>();
            summary.push_str(&format!("\n  {clipped}"));
        }
        if hidden > 0 {
            summary.push_str(&format!("\n  … {hidden} more diagnostic lines"));
        }
    }
    summary
}

/// Extract `session.sessionId` from a `session/create` result.
pub fn app_session_id_from_result(result: &serde_json::Value) -> Option<String> {
    result
        .pointer("/session/sessionId")
        .and_then(|v| v.as_str())
        .map(str::to_string)
}

/// Whether a `state.updated` marks the running turn as finished. The kernel
/// signals turn completion with `reason == "prompt_completed"` — there is no
/// `finish` session/event — so this is the authoritative turn terminator. A
/// status patch to the unambiguous terminal `completed` is a version-tolerant
/// fallback. `idle`/`ready` are deliberately NOT treated as turn-end: the
/// kernel can emit them as a settling state on a reused session *before* tokens
/// flow, which would finalize the turn prematurely as "(no output)".
pub fn app_state_is_turn_end(params: &serde_json::Value) -> bool {
    if params.get("reason").and_then(|r| r.as_str()) == Some("prompt_completed") {
        return true;
    }
    params.pointer("/patch/status").and_then(|s| s.as_str()) == Some("completed")
}

/// Whether a `state.updated` marks the turn as ended *abnormally* (error /
/// failed / aborted / cancelled / interrupted), via `reason` or `patch/status`.
/// Returns the offending word so the turn can be closed with a note instead of
/// hanging on a false "streaming" spinner until the 600s backstop fires.
pub fn app_state_turn_error(params: &serde_json::Value) -> Option<String> {
    fn is_bad(s: &str) -> bool {
        const BAD: [&str; 6] = [
            "error",
            "failed",
            "aborted",
            "cancelled",
            "canceled",
            "interrupted",
        ];
        BAD.contains(&s.to_ascii_lowercase().as_str())
    }
    for candidate in [
        params.get("reason").and_then(|r| r.as_str()),
        params.pointer("/patch/status").and_then(|s| s.as_str()),
    ]
    .into_iter()
    .flatten()
    {
        if is_bad(candidate) {
            return Some(candidate.to_string());
        }
    }
    None
}

/// Best-effort context watermark from a `state.updated` payload. The exact
/// JSON path is not contractual and shifts across kernel builds, so this
/// walks the tree for the first object carrying a used/window numeric pair
/// under any known key name. Missing/zero window -> None (no watermark, no
/// crash), which the caller treats as "leave the last value in place".
pub fn app_state_watermark(params: &serde_json::Value) -> Option<(u64, u64)> {
    const USED_KEYS: [&str; 4] = ["contextUsed", "used", "tokensUsed", "contextTokens"];
    const WINDOW_KEYS: [&str; 4] = ["contextWindow", "window", "total", "maxTokens"];
    fn walk(value: &serde_json::Value) -> Option<(u64, u64)> {
        match value {
            serde_json::Value::Object(map) => {
                let used = USED_KEYS
                    .iter()
                    .find_map(|key| map.get(*key).and_then(serde_json::Value::as_u64));
                let window = WINDOW_KEYS
                    .iter()
                    .find_map(|key| map.get(*key).and_then(serde_json::Value::as_u64));
                if let (Some(used), Some(window)) = (used, window) {
                    if window > 0 {
                        return Some((used, window));
                    }
                }
                map.values().find_map(walk)
            }
            serde_json::Value::Array(items) => items.iter().find_map(walk),
            _ => None,
        }
    }
    walk(params)
}

/// One tool invocation within a turn, correlated across its start/input/result
/// events by `call_id`. `finished` flips when the `result` event lands.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AppToolCall {
    pub call_id: String,
    pub name: String,
    /// Accumulated `tool_input_delta` JSON (the arguments).
    pub input: String,
    /// Tool output text (`result.result.content`).
    pub output: String,
    pub success: bool,
    pub duration_ms: Option<u64>,
    pub finished: bool,
}

/// What visibly changed when a turn applied one event — lets the UI know
/// exactly when to grow text, show a tool chip, or drop a finished tool into
/// the transcript, without re-diffing the whole turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TurnDelta {
    /// Nothing the UI needs to react to.
    None,
    /// Visible answer text grew.
    Text,
    /// Reasoning text grew (work-panel only).
    Reasoning,
    /// `tools[idx]` just began (show a running chip).
    ToolStarted(usize),
    /// `tools[idx]` just finished (persist its presentation to the transcript).
    ToolFinished(usize),
    /// The turn completed.
    Done,
}

/// Accumulates a streaming turn from session/event deltas. Body text arrives as
/// `text_delta` (like Anthropic content_block_delta); tool calls arrive as a
/// start/input/result sequence correlated by `toolCallId`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AppServerTurn {
    pub text: String,
    pub reasoning: String,
    pub tools: Vec<AppToolCall>,
    pub done: bool,
    /// `checkpoint.created` events seen this turn (one per gated tool write).
    pub checkpoints: u64,
    /// Sum of those events' `fileCount` — the turn's files-changed total.
    pub files_changed: u64,
}

impl AppServerTurn {
    fn tool_index(&self, call_id: &str) -> Option<usize> {
        self.tools.iter().position(|t| t.call_id == call_id)
    }

    /// Apply one event, returning what changed so the caller can react.
    pub fn apply(&mut self, event: &AppServerEvent) -> TurnDelta {
        match event.kind.as_str() {
            "text_delta" => {
                self.text.push_str(&event.delta);
                TurnDelta::Text
            }
            "reasoning_delta" => {
                self.reasoning.push_str(&event.delta);
                TurnDelta::Reasoning
            }
            // First sighting of a tool (start marker or full call) registers it.
            "tool_input_start" | "tool_call" => {
                let Some(call_id) = event.tool_call_id.as_deref() else {
                    return TurnDelta::None;
                };
                if let Some(idx) = self.tool_index(call_id) {
                    if let Some(name) = &event.tool_name {
                        if !name.is_empty() {
                            self.tools[idx].name = name.clone();
                        }
                    }
                    TurnDelta::None
                } else {
                    self.tools.push(AppToolCall {
                        call_id: call_id.to_string(),
                        name: event.tool_name.clone().unwrap_or_default(),
                        ..Default::default()
                    });
                    TurnDelta::ToolStarted(self.tools.len() - 1)
                }
            }
            "tool_input_delta" => {
                if let Some(call_id) = event.tool_call_id.as_deref() {
                    if let Some(idx) = self.tool_index(call_id) {
                        self.tools[idx].input.push_str(&event.delta);
                    }
                }
                TurnDelta::None
            }
            "result" => {
                let Some(call_id) = event.tool_call_id.as_deref() else {
                    return TurnDelta::None;
                };
                let Some(idx) = self.tool_index(call_id) else {
                    return TurnDelta::None;
                };
                let tool = &mut self.tools[idx];
                if let Some(output) = &event.output {
                    tool.output = output.clone();
                }
                tool.success = event.success.unwrap_or(true);
                tool.duration_ms = event.duration_ms;
                tool.finished = true;
                TurnDelta::ToolFinished(idx)
            }
            "finish" => {
                self.done = true;
                TurnDelta::Done
            }
            "text_end" if event.done => {
                self.done = true;
                TurnDelta::Done
            }
            // Session-level checkpoint (params.type passthrough): one per
            // gated tool write; fileCount sums into the turn's change total
            // for the finalize-time "N file(s) changed" note.
            "checkpoint.created" => {
                self.checkpoints += 1;
                self.files_changed += event.file_count.unwrap_or(0);
                TurnDelta::None
            }
            // input_end, scheduled, started, batch, tool_result, unknown: no-op.
            _ => TurnDelta::None,
        }
    }
}

/// Why the app-server path was abandoned (all trigger a --prompt fallback).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppServerUnavailable {
    Spawn(String),
    Handshake(String),
    Protocol(String),
    Disconnected,
}

impl std::fmt::Display for AppServerUnavailable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Spawn(why) => write!(f, "app-server did not start: {why}"),
            Self::Handshake(why) => write!(f, "app-server handshake failed: {why}"),
            Self::Protocol(why) => write!(f, "app-server protocol error: {why}"),
            Self::Disconnected => write!(f, "app-server connection closed"),
        }
    }
}

/// Whether prompts take the app-server streaming path. ON by default since
/// the graduation (streaming-graduation change): the path is a functional
/// superset of `--prompt` (true streaming + permission approval + session
/// controls + steer) and seamlessly downgrades on any failure. Only an
/// explicit opt-out disables it; `=1/true/on` stays accepted for the scripts
/// and wrappers written while it was opt-in.
pub fn app_server_enabled<F>(env_lookup: F) -> bool
where
    F: Fn(&str) -> Option<String>,
{
    !env_lookup("ZCODE_TUI_APP_SERVER").is_some_and(|value| {
        matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "0" | "off" | "false" | "no"
        )
    })
}

/// Append-only debug log, enabled by `ZCODE_TUI_LOG=<file path>`. Zero
/// overhead when unset (`from_env` is checked once per owner; the disabled
/// path is a single `is_none()`). Write failures are silently ignored —
/// diagnostics must never break the TUI.
///
/// REDACTION DISCIPLINE: outbound entries carry METHOD NAMES ONLY — request
/// params are never serialized (session/create·resume params embed
/// `runtimeModel` with the provider apiKey). Inbound summaries are truncated
/// and structural (class/kind/reason/id), never raw lines.
#[derive(Clone)]
pub struct DebugLog {
    file: Arc<Mutex<fs::File>>,
}

impl DebugLog {
    pub fn from_env() -> Option<Self> {
        let path = std::env::var_os("ZCODE_TUI_LOG")?;
        if path.is_empty() {
            return None;
        }
        let file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .ok()?;
        Some(Self {
            file: Arc::new(Mutex::new(file)),
        })
    }

    /// Append one timestamped line; failures are dropped on the floor.
    pub fn line(&self, text: &str) {
        if let Ok(mut file) = self.file.lock() {
            let ms = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis())
                .unwrap_or(0);
            let day_secs = (ms / 1000) % 86_400;
            let _ = writeln!(
                file,
                "{:02}:{:02}:{:02}.{:03} {text}",
                day_secs / 3600,
                (day_secs / 60) % 60,
                day_secs % 60,
                ms % 1000
            );
        }
    }
}

/// Outbound request log line: the method name and id, NOTHING else — params
/// stay out of the log by construction (see [`DebugLog`] redaction notes).
pub fn log_line_outbound(method: &str, id: u64) -> String {
    format!("-> {method} (id {id})")
}

/// Structural outbound request log. V4 commands add type/revision only; the
/// payload is deliberately never serialized because sendText contains the
/// user's prompt and other commands may grow credential-bearing fields.
pub fn log_line_outbound_request(method: &str, id: u64, params: &serde_json::Value) -> String {
    if method != "v4/command" {
        return log_line_outbound(method, id);
    }
    let kind = params.get("type").and_then(|v| v.as_str()).unwrap_or("-");
    let revision = params
        .get("baseRevision")
        .and_then(serde_json::Value::as_u64)
        .map(|value| value.to_string())
        .unwrap_or_else(|| "-".to_string());
    format!("-> v4/command type={kind} rev={revision} (id {id})")
}

/// Inbound message summary: message class + structural fields, truncated.
/// Result/params bodies are never serialized.
pub fn log_line_inbound(message: &AppServerMessage) -> String {
    match message {
        AppServerMessage::Response {
            id,
            error: Some(error),
            ..
        } => format!("<- response id {id} ERR {}", truncate_chars(error, 160)),
        AppServerMessage::Response { id, .. } => format!("<- response id {id} ok"),
        AppServerMessage::Event(event) => {
            let mut line = format!("<- event {}", event.kind);
            if !event.delta.is_empty() {
                line.push_str(&format!(" +{}b", event.delta.len()));
            }
            if let Some(name) = event.tool_name.as_deref().filter(|name| !name.is_empty()) {
                line.push_str(&format!(" tool={}", truncate_chars(name, 40)));
            }
            if let Some(count) = event.file_count {
                line.push_str(&format!(" files={count}"));
            }
            line
        }
        AppServerMessage::StateUpdated(params) => format!(
            "<- state.updated reason={}",
            params.get("reason").and_then(|r| r.as_str()).unwrap_or("-")
        ),
        AppServerMessage::V4Frame(params) => {
            let payload = params.pointer("/frame/payload");
            let kind = payload
                .and_then(|value| value.get("kind"))
                .and_then(|value| value.as_str())
                .unwrap_or("-");
            let revision = payload
                .and_then(|value| value.pointer("/snapshot/revision"))
                .and_then(serde_json::Value::as_u64)
                .or_else(|| {
                    payload
                        .and_then(|value| value.get("deltas"))
                        .and_then(|value| value.as_array())
                        .and_then(|deltas| {
                            deltas.iter().rev().find_map(|delta| {
                                delta
                                    .pointer("/patch/revision")
                                    .and_then(serde_json::Value::as_u64)
                            })
                        })
                })
                .map(|value| value.to_string())
                .unwrap_or_else(|| "-".to_string());
            format!("<- v4/conversation/frame kind={kind} rev={revision}")
        }
        AppServerMessage::ServerRequest { id, method, .. } => {
            format!(
                "<- server-request {method} id {}",
                truncate_chars(&id.to_string(), 40)
            )
        }
        AppServerMessage::Other => "<- other".to_string(),
    }
}

/// A long-lived connection to `zcode app-server`: one child process (own
/// process group), a reader thread decoding inbound lines, and a stdin
/// handle for requests. Requests get monotonic ids.
pub struct AppServerConn {
    child: Arc<Mutex<Child>>,
    stdin: std::process::ChildStdin,
    receiver: Receiver<AppServerMessage>,
    /// Messages read while blocking for a specific response id are stashed
    /// here so `poll` still delivers them afterwards (e.g. early events).
    pending: VecDeque<AppServerMessage>,
    next_id: u64,
    alive: bool,
    /// ZCODE_TUI_LOG debug log (None = disabled, zero overhead).
    log: Option<DebugLog>,
}

impl AppServerConn {
    pub fn spawn(zcode_bin: &str) -> std::result::Result<Self, AppServerUnavailable> {
        let mut process = Command::new(zcode_bin);
        process
            .arg("app-server")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            process.process_group(0);
        }
        let mut child = process
            .spawn()
            .map_err(|error| AppServerUnavailable::Spawn(error.to_string()))?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| AppServerUnavailable::Spawn("no stdin pipe".to_string()))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| AppServerUnavailable::Spawn("no stdout pipe".to_string()))?;
        let log = DebugLog::from_env();
        let reader_log = log.clone();
        let (sender, receiver) = channel();
        thread::spawn(move || {
            let reader = BufReader::new(stdout);
            for line in reader.lines() {
                let Ok(line) = line else { break };
                if let Some(message) = decode_app_message(&line) {
                    if let Some(log) = &reader_log {
                        log.line(&log_line_inbound(&message));
                    }
                    if sender.send(message).is_err() {
                        break;
                    }
                }
            }
        });
        if let Some(log) = &log {
            log.line("app-server spawned");
        }
        Ok(Self {
            child: Arc::new(Mutex::new(child)),
            stdin,
            receiver,
            pending: VecDeque::new(),
            next_id: 1,
            alive: true,
            log,
        })
    }

    fn write_request(
        &mut self,
        method: &str,
        params: serde_json::Value,
    ) -> std::result::Result<u64, AppServerUnavailable> {
        let id = self.next_id;
        self.next_id += 1;
        // Method name only — params may carry credentials (runtimeModel).
        if let Some(log) = &self.log {
            log.line(&log_line_outbound_request(method, id, &params));
        }
        let line = format!("{}\n", encode_app_request(id, method, params));
        self.stdin
            .write_all(line.as_bytes())
            .and_then(|()| self.stdin.flush())
            .map_err(|_| {
                self.alive = false;
                AppServerUnavailable::Disconnected
            })?;
        Ok(id)
    }

    /// Fire-and-forget request; the response arrives via `poll`.
    pub fn send(
        &mut self,
        method: &str,
        params: serde_json::Value,
    ) -> std::result::Result<u64, AppServerUnavailable> {
        self.write_request(method, params)
    }

    /// Write a pre-encoded line verbatim (plus framing newline). Replies to
    /// server→client requests echo the kernel's own envelope id — string ids
    /// like "server-1" — so they bypass the u64 request-id counter.
    pub fn reply(&mut self, line: &str) -> std::result::Result<(), AppServerUnavailable> {
        // Marker only — the reply body echoes kernel-provided payloads.
        if let Some(log) = &self.log {
            log.line("-> server-request reply");
        }
        self.stdin
            .write_all(format!("{line}\n").as_bytes())
            .and_then(|()| self.stdin.flush())
            .map_err(|_| {
                self.alive = false;
                AppServerUnavailable::Disconnected
            })
    }

    /// Send a request and block for its response, stashing any other
    /// messages that arrive meanwhile. Used for the fast create/subscribe
    /// handshake; the streaming turn uses `poll` instead.
    pub fn request_blocking(
        &mut self,
        method: &str,
        params: serde_json::Value,
        timeout: Duration,
    ) -> std::result::Result<serde_json::Value, AppServerUnavailable> {
        let want = self.write_request(method, params)?;
        let deadline = std::time::Instant::now() + timeout;
        loop {
            let remaining = deadline.saturating_duration_since(std::time::Instant::now());
            if remaining.is_zero() {
                return Err(AppServerUnavailable::Handshake(format!(
                    "{method} timed out"
                )));
            }
            match self.receiver.recv_timeout(remaining) {
                Ok(AppServerMessage::Response { id, result, error }) if id == want => {
                    return match error {
                        Some(message) => Err(AppServerUnavailable::Protocol(message)),
                        None => Ok(result.unwrap_or(serde_json::Value::Null)),
                    };
                }
                Ok(other) => self.pending.push_back(other),
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                    return Err(AppServerUnavailable::Handshake(format!(
                        "{method} timed out"
                    )));
                }
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                    self.alive = false;
                    return Err(AppServerUnavailable::Disconnected);
                }
            }
        }
    }

    /// Non-blocking: next buffered or newly-arrived message, if any.
    pub fn poll(&mut self) -> Option<AppServerMessage> {
        if let Some(message) = self.pending.pop_front() {
            return Some(message);
        }
        match self.receiver.try_recv() {
            Ok(message) => Some(message),
            Err(std::sync::mpsc::TryRecvError::Empty) => None,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.alive = false;
                None
            }
        }
    }

    pub fn is_alive(&self) -> bool {
        self.alive
    }

    pub fn cancel(&self) {
        if let Ok(mut child) = self.child.lock() {
            #[cfg(unix)]
            unsafe {
                libc::killpg(child.id() as libc::pid_t, libc::SIGKILL);
            }
            let _ = child.kill();
            // Reap the killed process so it does not linger as a <defunct>
            // zombie for the (possibly hours-long) rest of the TUI session.
            // After SIGKILL to the group this returns promptly.
            let _ = child.wait();
        }
    }
}

impl Drop for AppServerConn {
    fn drop(&mut self) {
        self.cancel();
    }
}
