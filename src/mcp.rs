use crate::domain::{self, Lesson, LessonPredicates, PredicateConditions};
use crate::privacy;
use crate::retrieval;
use crate::storage::{StorageError, Store};
use crate::time;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};

const INSTRUCTIONS: &str = "Use Reflex after you encounter one or more failed tool calls and later discover a reusable correction. Do not call it for every failure. Call register_repair_episode only after a repair succeeds or the user confirms the fix. Do not register a new lesson when a Reflex block already supplied the repair command. Never store secrets, tokens, private keys, session cookies, one-time URLs, or broad rules such as \"always use sudo.\"";

#[derive(Debug)]
pub enum McpError {
    Io(std::io::Error),
    Json(serde_json::Error),
    Storage(StorageError),
    InvalidInput(String),
}

impl std::fmt::Display for McpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(err) => write!(f, "io error: {err}"),
            Self::Json(err) => write!(f, "json error: {err}"),
            Self::Storage(err) => write!(f, "storage error: {err}"),
            Self::InvalidInput(err) => write!(f, "invalid input: {err}"),
        }
    }
}

impl std::error::Error for McpError {}

impl From<std::io::Error> for McpError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<serde_json::Error> for McpError {
    fn from(value: serde_json::Error) -> Self {
        Self::Json(value)
    }
}

impl From<StorageError> for McpError {
    fn from(value: StorageError) -> Self {
        Self::Storage(value)
    }
}

pub type McpResult<T> = Result<T, McpError>;

#[derive(Debug, Deserialize)]
struct JsonRpcRequest {
    id: Option<Value>,
    method: String,
    #[serde(default)]
    params: Value,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct RegisterRepairEpisodeInput {
    pub case_id: Option<String>,
    pub reusable: bool,
    pub failure_summary: String,
    pub repair_summary: String,
    pub lesson_hint: String,
    pub trigger_description: String,
    #[serde(default)]
    pub avoid_when: Vec<String>,
    pub predicates: LessonPredicates,
    pub scope: String,
    pub risk_level: String,
    pub confidence: f64,
}

pub fn run_stdio(input: &mut dyn Read, output: &mut dyn Write) -> McpResult<()> {
    let store = Store::open_default()?;
    let reader = BufReader::new(input);
    for line in reader.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let request: JsonRpcRequest = serde_json::from_str(&line)?;
        let response = handle_json_rpc(&store, request)?;
        serde_json::to_writer(&mut *output, &response)?;
        writeln!(output)?;
        output.flush()?;
    }
    Ok(())
}

fn handle_json_rpc(store: &Store, request: JsonRpcRequest) -> McpResult<Value> {
    let id = request.id.clone().unwrap_or(Value::Null);
    let result = match request.method.as_str() {
        "initialize" => json!({
            "protocolVersion": "2024-11-05",
            "capabilities": {"tools": {}},
            "serverInfo": {"name": "reflex", "version": env!("CARGO_PKG_VERSION")},
            "instructions": INSTRUCTIONS,
        }),
        "tools/list" => json!({"tools": tool_list()}),
        "tools/call" => call_tool(store, &request.params)?,
        _ => {
            return Ok(json!({
                "jsonrpc": "2.0",
                "id": id,
                "error": {"code": -32601, "message": format!("unknown method {}", request.method)}
            }))
        }
    };
    Ok(json!({"jsonrpc": "2.0", "id": id, "result": result}))
}

pub fn register_repair_episode(
    store: &Store,
    input: RegisterRepairEpisodeInput,
) -> McpResult<Value> {
    let safe_input = RegisterRepairEpisodeInput {
        case_id: input.case_id.map(|id| privacy::redact_text(&id)),
        reusable: input.reusable,
        failure_summary: privacy::redact_text(&input.failure_summary),
        repair_summary: privacy::redact_text(&input.repair_summary),
        lesson_hint: privacy::redact_text(&input.lesson_hint),
        trigger_description: privacy::redact_text(&input.trigger_description),
        avoid_when: input
            .avoid_when
            .iter()
            .map(|item| privacy::redact_text(item))
            .collect(),
        predicates: redact_predicates(input.predicates),
        scope: input.scope,
        risk_level: input.risk_level,
        confidence: input.confidence.clamp(0.0, 1.0),
    };
    if !safe_input.reusable {
        return Ok(json!({
            "lesson_id": null,
            "status": "not_reusable",
            "message": "Reflex ignored this repair because reusable=false."
        }));
    }
    validate_predicates(&safe_input.predicates)?;

    let case = safe_input
        .case_id
        .as_deref()
        .and_then(|id| store.get_episode(id).ok().flatten());
    let project_hash = case
        .as_ref()
        .map(|case| case.project_hash.clone())
        .unwrap_or_else(|| {
            domain::project_hash(
                &registration_project_cwd(&safe_input).unwrap_or_else(current_dir_text),
            )
        });
    if let Some(existing) = duplicate_lesson(store, &project_hash, &safe_input)? {
        return Ok(json!({
            "lesson_id": existing.id,
            "status": existing.status,
            "message": "A similar Reflex lesson already exists for this project."
        }));
    }
    let now = time::now_text();
    let lesson_id = domain::new_id("lesson");
    let failed_attempt_ids = case
        .as_ref()
        .map(|case| case.attempt_ids.clone())
        .unwrap_or_default();
    let lesson = Lesson {
        id: lesson_id.clone(),
        project_hash,
        status: "candidate".to_string(),
        scope_json: json!({
            "level": safe_input.scope,
        }),
        trigger_json: json!({
            "description": safe_input.trigger_description,
            "positive_terms": term_list(&format!("{} {}", safe_input.trigger_description, safe_input.lesson_hint)),
            "negative_terms": safe_input
                .avoid_when
                .iter()
                .map(|item| item.to_ascii_lowercase())
                .collect::<Vec<_>>(),
            "avoid_when": safe_input.avoid_when,
            "predicates": safe_input.predicates,
        }),
        lesson_json: json!({
            "hint": safe_input.lesson_hint,
            "repair_summary": safe_input.repair_summary,
            "failure_summary": safe_input.failure_summary,
            "risk_level": safe_input.risk_level,
            "rewrite_allowed": false,
        }),
        evidence_json: json!({
            "case_ids": safe_input.case_id.iter().collect::<Vec<_>>(),
            "failed_attempt_ids": failed_attempt_ids,
            "model_registered": true,
        }),
        confidence: safe_input.confidence,
        times_injected: 0,
        times_confirmed: 1,
        times_contradicted: 0,
        created_at: now.clone(),
        updated_at: now.clone(),
        last_injected_at: None,
        last_confirmed_at: Some(now),
    };
    store.insert_lesson(&lesson)?;
    if let Some(mut case) = case {
        case.status = domain::EpisodeStatus::ModelRegistered;
        case.updated_at = time::now_text();
        case.resolution_json =
            Some(json!({"lesson_id": lesson_id, "source": "mcp_register_repair_episode"}));
        store.update_episode(&case)?;
    }
    Ok(json!({
        "lesson_id": lesson_id,
        "status": "candidate",
        "message": "Recorded candidate Reflex lesson. It will be injected only on close matches until confirmed."
    }))
}

fn registration_project_cwd(input: &RegisterRepairEpisodeInput) -> Option<String> {
    let text = format!(
        "{} {} {} {}",
        input.trigger_description, input.lesson_hint, input.failure_summary, input.repair_summary
    );
    first_existing_absolute_path(&text).map(|path| path.to_string_lossy().into_owned())
}

fn first_existing_absolute_path(text: &str) -> Option<PathBuf> {
    for token in text.split_whitespace() {
        if let Some(start) = token.find('/') {
            let candidate = token[start..].trim_matches(|ch| {
                matches!(
                    ch,
                    '`' | '\'' | '"' | ',' | '.' | ';' | ':' | ')' | ']' | '}' | '(' | '[' | '{'
                )
            });
            let path = Path::new(candidate);
            if path.is_absolute() && path.exists() {
                return Some(path.to_path_buf());
            }
        }
    }
    None
}

fn current_dir_text() -> String {
    std::env::current_dir()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned()
}

fn redact_predicates(predicates: LessonPredicates) -> LessonPredicates {
    LessonPredicates {
        tool_family: predicates
            .tool_family
            .map(|value| privacy::redact_text(&value)),
        command_family: predicates
            .command_family
            .map(|value| privacy::redact_text(&value)),
        match_executables: predicates
            .match_executables
            .iter()
            .map(|value| privacy::redact_text(value))
            .collect(),
        required_env: predicates
            .required_env
            .iter()
            .map(|(key, value)| (privacy::redact_text(key), privacy::redact_text(value)))
            .collect(),
        required_cwd: predicates
            .required_cwd
            .map(|value| privacy::redact_text(&value)),
        preferred_command: predicates
            .preferred_command
            .map(|value| privacy::redact_text(&value)),
        suppress_when: redact_conditions(predicates.suppress_when),
        block_when: redact_conditions(predicates.block_when),
    }
}

fn validate_predicates(predicates: &LessonPredicates) -> McpResult<()> {
    if predicates.command_family.is_none() && predicates.match_executables.is_empty() {
        return Err(McpError::InvalidInput(
            "predicates must include command_family or match_executables".to_string(),
        ));
    }
    Ok(())
}

fn redact_conditions(conditions: PredicateConditions) -> PredicateConditions {
    PredicateConditions {
        env: conditions
            .env
            .iter()
            .map(|(key, value)| (privacy::redact_text(key), privacy::redact_text(value)))
            .collect(),
        command_contains: conditions
            .command_contains
            .iter()
            .map(|value| privacy::redact_text(value))
            .collect(),
        executable: conditions
            .executable
            .map(|value| privacy::redact_text(&value)),
    }
}

fn duplicate_lesson(
    store: &Store,
    project_hash: &str,
    input: &RegisterRepairEpisodeInput,
) -> McpResult<Option<Lesson>> {
    let input_text = registration_text(input);
    let input_env = env_assignments(&input_text);
    let input_intents = command_intents(&input_text);
    for lesson in store.lessons_for_project(project_hash)? {
        let existing_text = format!(
            "{} {} {}",
            lesson.trigger_json, lesson.lesson_json, lesson.scope_json
        );
        let existing_env = env_assignments(&existing_text);
        let existing_intents = command_intents(&existing_text);
        if !input_env.is_empty()
            && input_env == existing_env
            && !input_intents.is_disjoint(&existing_intents)
        {
            return Ok(Some(lesson));
        }
        if !input_intents.is_empty()
            && !input_intents.is_disjoint(&existing_intents)
            && token_similarity(&input_text, &existing_text) >= 0.55
        {
            return Ok(Some(lesson));
        }
    }
    Ok(None)
}

fn registration_text(input: &RegisterRepairEpisodeInput) -> String {
    format!(
        "{} {} {} {} {}",
        input.trigger_description,
        input.lesson_hint,
        input.failure_summary,
        input.repair_summary,
        input.avoid_when.join(" ")
    )
}

fn token_similarity(left: &str, right: &str) -> f64 {
    let left = token_set(left);
    let right = token_set(right);
    if left.is_empty() || right.is_empty() {
        return 0.0;
    }
    let intersection = left.intersection(&right).count() as f64;
    let union = left.union(&right).count() as f64;
    intersection / union
}

fn command_intents(text: &str) -> BTreeSet<String> {
    token_set(text)
        .into_iter()
        .filter(|token| COMMAND_INTENTS.contains(&token.as_str()))
        .collect()
}

fn env_assignments(text: &str) -> BTreeSet<String> {
    let mut assignments = BTreeSet::new();
    for raw in text.split_whitespace() {
        let token = raw.trim_matches(|ch: char| {
            matches!(
                ch,
                '`' | '\'' | '"' | ',' | '.' | ';' | ':' | ')' | ']' | '}' | '(' | '[' | '{'
            )
        });
        let Some((key, value)) = token.split_once('=') else {
            continue;
        };
        if key
            .chars()
            .all(|ch| ch.is_ascii_uppercase() || ch.is_ascii_digit() || ch == '_')
            && !value.is_empty()
        {
            assignments.insert(format!("{key}={value}"));
        }
    }
    assignments
}

fn token_set(text: &str) -> BTreeSet<String> {
    text.split(|ch: char| !ch.is_ascii_alphanumeric() && ch != '_' && ch != '-')
        .map(|term| term.to_ascii_lowercase())
        .filter(|term| term.len() >= 2 && !TOKEN_STOP_WORDS.contains(&term.as_str()))
        .collect()
}

fn call_tool(store: &Store, params: &Value) -> McpResult<Value> {
    let name = params
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    let arguments = params
        .get("arguments")
        .cloned()
        .unwrap_or_else(|| json!({}));
    let content = match name {
        "register_repair_episode" => {
            let input: RegisterRepairEpisodeInput = serde_json::from_value(arguments)?;
            register_repair_episode(store, input)?
        }
        "find_lessons" => find_lessons(store, &arguments)?,
        "mark_lesson_result" => mark_lesson_result(store, &arguments)?,
        "list_recent_cases" => list_recent_cases(store, &arguments)?,
        "ignore_lesson" => ignore_lesson(store, &arguments)?,
        _ => json!({"error": format!("unknown Reflex tool: {name}")}),
    };
    Ok(json!({"content": [{"type": "text", "text": content.to_string()}]}))
}

fn find_lessons(store: &Store, args: &Value) -> McpResult<Value> {
    let query = args
        .get("query")
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    let tool_name = args
        .get("tool_name")
        .and_then(|v| v.as_str())
        .unwrap_or("Bash");
    let cwd = args
        .get("cwd")
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .or_else(|| {
            std::env::current_dir()
                .ok()
                .map(|p| p.to_string_lossy().to_string())
        })
        .unwrap_or_else(|| ".".to_string());
    let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(5) as usize;
    let pending = domain::PendingTool {
        session_id: "mcp-query".to_string(),
        turn_id: None,
        cwd: cwd.clone(),
        project_hash: domain::project_hash(&cwd),
        tool_name: tool_name.to_string(),
        tool_input_json: json!({"query": query}),
        summary: format!("{tool_name} cwd={cwd} {query}"),
    };
    let lessons = retrieval::find_matches(store, &pending, limit)?
        .into_iter()
        .map(|matched| {
            json!({
                "lesson_id": matched.lesson.id,
                "status": matched.lesson.status,
                "hint": matched.lesson.lesson_json.get("hint").and_then(|v| v.as_str()).unwrap_or_default(),
                "confidence": matched.lesson.confidence,
                "scope": matched.lesson.scope_json.get("level").and_then(|v| v.as_str()).unwrap_or("project"),
            })
        })
        .collect::<Vec<_>>();
    Ok(json!({"lessons": lessons}))
}

fn mark_lesson_result(store: &Store, args: &Value) -> McpResult<Value> {
    let lesson_id = args
        .get("lesson_id")
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    let result = args
        .get("result")
        .and_then(|v| v.as_str())
        .unwrap_or("irrelevant");
    store.mark_lesson_result(lesson_id, result)?;
    let status = store
        .get_lesson(lesson_id)?
        .map(|lesson| lesson.status)
        .unwrap_or_else(|| "unknown".to_string());
    Ok(json!({"lesson_id": lesson_id, "status": status}))
}

fn list_recent_cases(store: &Store, args: &Value) -> McpResult<Value> {
    let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(10) as usize;
    let status = args.get("status").and_then(|v| v.as_str());
    let cases = store
        .list_cases(status, limit)?
        .into_iter()
        .map(|case| {
            json!({
                "case_id": case.id,
                "status": case.status.as_str(),
                "attempt_ids": case.attempt_ids,
                "created_at": case.created_at,
                "updated_at": case.updated_at,
            })
        })
        .collect::<Vec<_>>();
    Ok(json!({"cases": cases}))
}

fn ignore_lesson(store: &Store, args: &Value) -> McpResult<Value> {
    let lesson_id = args
        .get("lesson_id")
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    store.update_lesson_status(lesson_id, "ignored")?;
    Ok(json!({"lesson_id": lesson_id, "status": "ignored"}))
}

fn tool_list() -> Vec<Value> {
    vec![
        json!({
            "name": "register_repair_episode",
            "description": "Record a reusable operational repair after a failed tool call is successfully fixed.",
            "inputSchema": {
                "type": "object",
                "additionalProperties": false,
                "required": ["case_id", "reusable", "failure_summary", "repair_summary", "lesson_hint", "trigger_description", "avoid_when", "predicates", "scope", "risk_level", "confidence"],
                "properties": {
                    "case_id": {"type": ["string", "null"]},
                    "reusable": {"type": "boolean"},
                    "failure_summary": {"type": "string", "maxLength": 800},
                    "repair_summary": {"type": "string", "maxLength": 800},
                    "lesson_hint": {"type": "string", "maxLength": 240},
                    "trigger_description": {"type": "string", "maxLength": 500},
                    "avoid_when": {"type": "array", "items": {"type": "string", "maxLength": 240}, "maxItems": 5},
                    "predicates": {
                        "type": "object",
                        "additionalProperties": false,
                        "properties": {
                            "tool_family": {"type": ["string", "null"], "description": "Tool family this lesson applies to, such as Bash."},
                            "command_family": {"type": ["string", "null"], "description": "Stable command family, such as pytest, npm, cargo, or aws."},
                            "match_executables": {"type": "array", "items": {"type": "string"}, "maxItems": 8},
                            "required_env": {"type": "object", "additionalProperties": {"type": "string"}},
                            "required_cwd": {"type": ["string", "null"]},
                            "preferred_command": {"type": ["string", "null"]},
                            "suppress_when": {
                                "type": "object",
                                "additionalProperties": false,
                                "properties": {
                                    "env": {"type": "object", "additionalProperties": {"type": "string"}},
                                    "command_contains": {"type": "array", "items": {"type": "string"}, "maxItems": 8},
                                    "executable": {"type": ["string", "null"]}
                                }
                            },
                            "block_when": {
                                "type": "object",
                                "additionalProperties": false,
                                "properties": {
                                    "env": {"type": "object", "additionalProperties": {"type": "string"}},
                                    "command_contains": {"type": "array", "items": {"type": "string"}, "maxItems": 8},
                                    "executable": {"type": ["string", "null"]}
                                }
                            }
                        }
                    },
                    "scope": {"type": "string", "enum": ["project", "repo", "machine", "user", "org", "unknown"]},
                    "risk_level": {"type": "string", "enum": ["low", "medium", "high"]},
                    "confidence": {"type": "number", "minimum": 0, "maximum": 1}
                }
            }
        }),
        json!({"name": "find_lessons", "description": "Find Reflex lessons relevant to a tool or failure context.", "inputSchema": {"type": "object"}}),
        json!({"name": "mark_lesson_result", "description": "Confirm, contradict, or mark a lesson irrelevant.", "inputSchema": {"type": "object"}}),
        json!({"name": "list_recent_cases", "description": "List recent Reflex repair cases.", "inputSchema": {"type": "object"}}),
        json!({"name": "ignore_lesson", "description": "Disable a bad Reflex lesson.", "inputSchema": {"type": "object"}}),
    ]
}

pub fn tool_metadata() -> Vec<Value> {
    tool_list()
}

fn term_list(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_ascii_alphanumeric() && c != '_' && c != '-')
        .map(|term| term.to_ascii_lowercase())
        .filter(|term| term.len() >= 2)
        .take(20)
        .collect()
}

const COMMAND_INTENTS: &[&str] = &[
    "aws", "cargo", "docker", "eks", "git", "go", "kubectl", "make", "node", "npm", "pnpm",
    "poetry", "pytest", "python", "test", "uv",
];

const TOKEN_STOP_WORDS: &[&str] = &[
    "the",
    "and",
    "for",
    "with",
    "from",
    "this",
    "that",
    "use",
    "run",
    "previously",
    "similar",
];
