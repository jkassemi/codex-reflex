use crate::domain::{self, AttemptResult, EpisodeStatus, PendingTool, RepairEpisode, ToolAttempt};
use crate::privacy;
use crate::retrieval;
use crate::storage::{StorageError, Store};
use crate::time;
use serde_json::{json, Value};
use std::io::{Read, Write};

#[derive(Debug)]
pub enum HookError {
    Io(std::io::Error),
    Json(serde_json::Error),
    Storage(StorageError),
}

impl std::fmt::Display for HookError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(err) => write!(f, "io error: {err}"),
            Self::Json(err) => write!(f, "json error: {err}"),
            Self::Storage(err) => write!(f, "storage error: {err}"),
        }
    }
}

impl std::error::Error for HookError {}

impl From<std::io::Error> for HookError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<serde_json::Error> for HookError {
    fn from(value: serde_json::Error) -> Self {
        Self::Json(value)
    }
}

impl From<StorageError> for HookError {
    fn from(value: StorageError) -> Self {
        Self::Storage(value)
    }
}

pub type HookResult<T> = Result<T, HookError>;

pub fn run_hook(name: &str, input: &mut dyn Read, output: &mut dyn Write) -> HookResult<()> {
    let event = read_event(input)?;
    let store = Store::open_default()?;
    let response = match name {
        "session-start" => session_start(&store, &event)?,
        "user-prompt-submit" => user_prompt_submit(&store, &event)?,
        "pre-tool-use" => pre_tool_use(&store, &event)?,
        "post-tool-use" => post_tool_use(&store, &event)?,
        "permission-request" => permission_request(&store, &event)?,
        "stop" => stop(&store, &event)?,
        other => json!({"error": format!("unknown Reflex hook: {other}")}),
    };
    serde_json::to_writer(output, &response)?;
    Ok(())
}

pub fn session_start(_store: &Store, _event: &Value) -> HookResult<Value> {
    Ok(additional_context(
        "SessionStart",
        "Reflex is available: after a failed tool call is repaired, register reusable operational fixes with the Reflex MCP tool. Do not register a new lesson when a Reflex block already supplied the repair command.",
    ))
}

pub fn user_prompt_submit(_store: &Store, _event: &Value) -> HookResult<Value> {
    Ok(json!({}))
}

pub fn pre_tool_use(store: &Store, event: &Value) -> HookResult<Value> {
    let pending = pending_tool_from_event(event);
    let matches = retrieval::find_matches(store, &pending, 2)?;
    if matches.is_empty() {
        return Ok(json!({}));
    }
    for matched in &matches {
        store.record_injection(
            &matched.lesson.id,
            None,
            &pending.session_id,
            &retrieval::pending_summary_json(&pending),
        )?;
    }
    if let Some(reason) = retrieval::block_reason(&matches) {
        return Ok(json!({
            "decision": "block",
            "reason": reason
        }));
    }
    let context = retrieval::injection_context(&matches);
    Ok(additional_context("PreToolUse", &context))
}

pub fn post_tool_use(store: &Store, event: &Value) -> HookResult<Value> {
    let attempt = attempt_from_event(event, "PostToolUse");
    store.insert_attempt(&attempt)?;
    match attempt.result {
        AttemptResult::Failure => record_failure(store, &attempt),
        AttemptResult::Success => record_success(store, &attempt),
        AttemptResult::Unknown => Ok(json!({})),
    }
}

pub fn permission_request(store: &Store, event: &Value) -> HookResult<Value> {
    let mut attempt = attempt_from_event(event, "PermissionRequest");
    attempt.result = AttemptResult::Unknown;
    attempt.failure_kind = Some("permission".to_string());
    store.insert_attempt(&attempt)?;
    Ok(json!({}))
}

pub fn stop(store: &Store, _event: &Value) -> HookResult<Value> {
    let _open_cases = store.list_cases(Some("open"), 100)?;
    Ok(json!({}))
}

fn record_failure(store: &Store, attempt: &ToolAttempt) -> HookResult<Value> {
    if let Some(mut episode) =
        store.find_open_episode(&attempt.session_id, &attempt.project_hash)?
    {
        if !episode.attempt_ids.contains(&attempt.id) && episode.attempt_ids.len() < 25 {
            episode.attempt_ids.push(attempt.id.clone());
        }
        episode.status = EpisodeStatus::Repairing;
        episode.updated_at = time::now_text();
        store.update_episode(&episode)?;
        return Ok(json!({}));
    }

    let now = time::now_text();
    let episode = RepairEpisode {
        id: domain::new_id("case"),
        session_id: attempt.session_id.clone(),
        turn_id: attempt.turn_id.clone(),
        project_hash: attempt.project_hash.clone(),
        status: EpisodeStatus::Open,
        user_intent_excerpt: None,
        opened_by_attempt_id: attempt.id.clone(),
        attempt_ids: vec![attempt.id.clone()],
        created_at: now.clone(),
        updated_at: now,
        expires_at: time::future_text(2),
        resolution_json: None,
    };
    let case_id = episode.id.clone();
    store.insert_episode(&episode)?;
    Ok(additional_context(
        "PostToolUse",
        &format!("Reflex: failure recorded as case {case_id}. If you later find a reusable correction, call `register_repair_episode` with that case id."),
    ))
}

fn record_success(store: &Store, attempt: &ToolAttempt) -> HookResult<Value> {
    if let Some(mut episode) =
        store.find_open_episode(&attempt.session_id, &attempt.project_hash)?
    {
        if !episode.attempt_ids.contains(&attempt.id) && episode.attempt_ids.len() < 25 {
            episode.attempt_ids.push(attempt.id.clone());
        }
        episode.status = EpisodeStatus::CandidateRepaired;
        episode.updated_at = time::now_text();
        episode.resolution_json = Some(json!({
            "candidate_success_attempt_id": attempt.id,
            "source": "post_tool_use_success_after_failure",
        }));
        store.update_episode(&episode)?;
    }
    Ok(json!({}))
}

fn attempt_from_event(event: &Value, fallback_tool_name: &str) -> ToolAttempt {
    let tool_name = text_at(event, &["tool_name"])
        .or_else(|| text_at(event, &["toolName"]))
        .unwrap_or_else(|| fallback_tool_name.to_string());
    let tool_input = event
        .get("tool_input")
        .or_else(|| event.get("toolInput"))
        .cloned()
        .unwrap_or_else(|| json!({}));
    let tool_response = event
        .get("tool_response")
        .or_else(|| event.get("toolResponse"))
        .cloned()
        .unwrap_or_else(|| event.clone());
    let cwd = text_at(event, &["cwd"])
        .or_else(|| text_at(&tool_input, &["cwd"]))
        .or_else(|| {
            std::env::current_dir()
                .ok()
                .map(|p| p.to_string_lossy().to_string())
        })
        .unwrap_or_else(|| ".".to_string());
    let result = classify_result(&tool_response);
    ToolAttempt {
        id: domain::new_id("attempt"),
        session_id: text_at(event, &["session_id"])
            .or_else(|| text_at(event, &["sessionId"]))
            .unwrap_or_else(|| "unknown-session".to_string()),
        turn_id: text_at(event, &["turn_id"]).or_else(|| text_at(event, &["turnId"])),
        tool_use_id: text_at(event, &["tool_use_id"]).or_else(|| text_at(event, &["toolUseId"])),
        ts: time::now_text(),
        cwd: cwd.clone(),
        project_hash: domain::project_hash(&cwd),
        tool_name,
        tool_input_json: privacy::redact_value(&tool_input),
        tool_response_summary_json: summarize_response(&tool_response),
        result: result.0,
        failure_kind: result.1,
        raw_event_path: None,
    }
}

fn pending_tool_from_event(event: &Value) -> PendingTool {
    let tool_name = text_at(event, &["tool_name"])
        .or_else(|| text_at(event, &["toolName"]))
        .unwrap_or_else(|| "unknown".to_string());
    let tool_input = event
        .get("tool_input")
        .or_else(|| event.get("toolInput"))
        .cloned()
        .unwrap_or_else(|| json!({}));
    let cwd = text_at(event, &["cwd"])
        .or_else(|| text_at(&tool_input, &["cwd"]))
        .or_else(|| {
            std::env::current_dir()
                .ok()
                .map(|p| p.to_string_lossy().to_string())
        })
        .unwrap_or_else(|| ".".to_string());
    let redacted = privacy::redact_value(&tool_input);
    PendingTool {
        session_id: text_at(event, &["session_id"])
            .or_else(|| text_at(event, &["sessionId"]))
            .unwrap_or_else(|| "unknown-session".to_string()),
        turn_id: text_at(event, &["turn_id"]).or_else(|| text_at(event, &["turnId"])),
        cwd: cwd.clone(),
        project_hash: domain::project_hash(&cwd),
        tool_name: tool_name.clone(),
        summary: retrieval::pending_summary(&tool_name, &redacted, &cwd),
        tool_input_json: redacted,
    }
}

fn summarize_response(response: &Value) -> Value {
    let redacted = privacy::redact_value(response);
    let stdout = text_for_key(&redacted, &["stdout"]).unwrap_or_else(|| {
        if classify_result(response).0 == AttemptResult::Success {
            text_for_key(&redacted, &["output"]).unwrap_or_default()
        } else {
            String::new()
        }
    });
    let stderr = text_for_key(&redacted, &["stderr"])
        .or_else(|| text_for_key(&redacted, &["error"]))
        .unwrap_or_else(|| {
            if classify_result(response).0 == AttemptResult::Failure {
                text_for_key(&redacted, &["output"]).unwrap_or_default()
            } else {
                String::new()
            }
        });
    json!({
        "exit_code": exit_code(response),
        "result": classify_result(response).0.as_str(),
        "stdout_excerpt": truncate(&stdout, 2000),
        "stderr_excerpt": truncate(&stderr, 4000),
    })
}

fn classify_result(response: &Value) -> (AttemptResult, Option<String>) {
    if let Some(code) = exit_code(response) {
        if code == 0 {
            return (AttemptResult::Success, None);
        }
        return (AttemptResult::Failure, Some(classify_failure(response)));
    }
    if response.get("isError").and_then(|v| v.as_bool()) == Some(true)
        || response.get("error").is_some()
        || response.get("failed").and_then(|v| v.as_bool()) == Some(true)
    {
        return (AttemptResult::Failure, Some(classify_failure(response)));
    }
    if response.get("success").and_then(|v| v.as_bool()) == Some(true) {
        return (AttemptResult::Success, None);
    }
    (AttemptResult::Unknown, None)
}

fn classify_failure(response: &Value) -> String {
    let text = response.to_string().to_ascii_lowercase();
    if text.contains("permission denied") || text.contains("approval") {
        "permission".to_string()
    } else if text.contains("unauthorized") || text.contains("credentials") || text.contains("auth")
    {
        "auth".to_string()
    } else if text.contains("command not found") {
        "missing_dependency".to_string()
    } else if text.contains("not found") || text.contains("no such file") || text.contains("cwd") {
        "wrong_cwd".to_string()
    } else if text.contains("network") || text.contains("timeout") || text.contains("dns") {
        "network".to_string()
    } else if text.contains("assertion") || text.contains("failed") {
        "test_failure".to_string()
    } else {
        "unknown".to_string()
    }
}

fn read_event(input: &mut dyn Read) -> HookResult<Value> {
    let mut text = String::new();
    input.read_to_string(&mut text)?;
    if text.trim().is_empty() {
        return Ok(json!({}));
    }
    Ok(serde_json::from_str(&text)?)
}

fn additional_context(hook_event_name: &str, text: &str) -> Value {
    json!({
        "hookSpecificOutput": {
            "hookEventName": hook_event_name,
            "additionalContext": text
        }
    })
}

fn text_at(value: &Value, path: &[&str]) -> Option<String> {
    let mut current = value;
    for part in path {
        current = current.get(*part)?;
    }
    current.as_str().map(str::to_string)
}

fn numeric_at(value: &Value, path: &[&str]) -> Option<i64> {
    let mut current = value;
    for part in path {
        current = current.get(*part)?;
    }
    numeric_value(current)
}

fn exit_code(value: &Value) -> Option<i64> {
    numeric_at(value, &["exit_code"])
        .or_else(|| numeric_at(value, &["exitCode"]))
        .or_else(|| numeric_at(value, &["metadata", "exit_code"]))
        .or_else(|| numeric_at(value, &["metadata", "exitCode"]))
        .or_else(|| numeric_for_key(value, &["exit_code", "exitCode"]))
        .or_else(|| process_exit_code(value))
}

fn numeric_for_key(value: &Value, keys: &[&str]) -> Option<i64> {
    match value {
        Value::Object(map) => {
            for key in keys {
                if let Some(number) = map.get(*key).and_then(numeric_value) {
                    return Some(number);
                }
            }
            map.values().find_map(|child| numeric_for_key(child, keys))
        }
        Value::Array(items) => items.iter().find_map(|child| numeric_for_key(child, keys)),
        _ => None,
    }
}

fn text_for_key(value: &Value, keys: &[&str]) -> Option<String> {
    match value {
        Value::Object(map) => {
            for key in keys {
                if let Some(text) = map.get(*key).and_then(string_value) {
                    return Some(text);
                }
            }
            map.values().find_map(|child| text_for_key(child, keys))
        }
        Value::Array(items) => items.iter().find_map(|child| text_for_key(child, keys)),
        _ => None,
    }
}

fn process_exit_code(value: &Value) -> Option<i64> {
    match value {
        Value::String(text) => parse_process_exit_code(text),
        Value::Object(map) => map.values().find_map(process_exit_code),
        Value::Array(items) => items.iter().find_map(process_exit_code),
        _ => None,
    }
}

fn parse_process_exit_code(text: &str) -> Option<i64> {
    let marker = "Process exited with code ";
    let start = text.find(marker)? + marker.len();
    let digits = text[start..]
        .chars()
        .take_while(|ch| ch.is_ascii_digit() || *ch == '-')
        .collect::<String>();
    if digits.is_empty() {
        None
    } else {
        digits.parse().ok()
    }
}

fn numeric_value(value: &Value) -> Option<i64> {
    value
        .as_i64()
        .or_else(|| value.as_str().and_then(|text| text.parse().ok()))
}

fn string_value(value: &Value) -> Option<String> {
    value.as_str().map(str::to_string)
}

fn truncate(input: &str, max: usize) -> String {
    if input.len() <= max {
        input.to_string()
    } else {
        let mut text = input.to_string();
        text.truncate(max);
        text
    }
}
