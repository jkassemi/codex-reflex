use crate::domain;
use crate::hooks;
use crate::mcp::{self, RegisterRepairEpisodeInput};
use crate::storage::{StorageError, Store};
use serde_json::json;
use std::io::{Read, Write};

#[derive(Debug)]
pub enum CliError {
    Io(std::io::Error),
    Json(serde_json::Error),
    Storage(StorageError),
    Hook(hooks::HookError),
    Mcp(mcp::McpError),
    Usage(String),
}

impl std::fmt::Display for CliError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(err) => write!(f, "io error: {err}"),
            Self::Json(err) => write!(f, "json error: {err}"),
            Self::Storage(err) => write!(f, "storage error: {err}"),
            Self::Hook(err) => write!(f, "hook error: {err}"),
            Self::Mcp(err) => write!(f, "mcp error: {err}"),
            Self::Usage(text) => write!(f, "{text}"),
        }
    }
}

impl std::error::Error for CliError {}

impl From<std::io::Error> for CliError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<serde_json::Error> for CliError {
    fn from(value: serde_json::Error) -> Self {
        Self::Json(value)
    }
}

impl From<StorageError> for CliError {
    fn from(value: StorageError) -> Self {
        Self::Storage(value)
    }
}

impl From<hooks::HookError> for CliError {
    fn from(value: hooks::HookError) -> Self {
        Self::Hook(value)
    }
}

impl From<mcp::McpError> for CliError {
    fn from(value: mcp::McpError) -> Self {
        Self::Mcp(value)
    }
}

pub type CliResult<T> = Result<T, CliError>;

pub fn run<I>(args: I, input: &mut dyn Read, output: &mut dyn Write) -> CliResult<()>
where
    I: IntoIterator<Item = String>,
{
    let args = args.into_iter().collect::<Vec<_>>();
    match args.first().map(String::as_str) {
        Some("hook") => {
            let hook_name = args.get(1).ok_or_else(|| CliError::Usage(usage()))?;
            hooks::run_hook(hook_name, input, output)?;
        }
        Some("status") => status(output)?,
        Some("lessons") => lessons(output)?,
        Some("lesson") => lesson(args.get(1), output)?,
        Some("cases") => cases(output)?,
        Some("case") => case(args.get(1), output)?,
        Some("ignore") => update_lesson(args.get(1), "ignored", output)?,
        Some("promote") => update_lesson(args.get(1), "active", output)?,
        Some("demote") => update_lesson(args.get(1), "candidate", output)?,
        Some("analyze") => analyze(output)?,
        Some("doctor") => doctor(output)?,
        Some("export") => export(output)?,
        Some("purge") => purge(&args[1..], output)?,
        Some("register-demo-lesson") => register_demo_lesson(output)?,
        _ => return Err(CliError::Usage(usage())),
    }
    Ok(())
}

fn status(output: &mut dyn Write) -> CliResult<()> {
    let store = Store::open_default()?;
    let stats = store.stats()?;
    writeln!(output, "Reflex data: {}", store.root().display())?;
    writeln!(output, "Attempts: {}", stats.attempts)?;
    writeln!(output, "Cases: {}", stats.cases)?;
    writeln!(output, "Lessons: {}", stats.lessons)?;
    writeln!(output, "Injections: {}", stats.injections)?;
    writeln!(output, "Storage bytes: {}", stats.db_bytes)?;
    Ok(())
}

fn lessons(output: &mut dyn Write) -> CliResult<()> {
    let store = Store::open_default()?;
    writeln!(output, "Reflex lessons:")?;
    for lesson in store.list_lessons(100)? {
        let hint = lesson
            .lesson_json
            .get("hint")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let trigger = lesson
            .trigger_json
            .get("description")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        writeln!(
            output,
            "{}  confidence {:.2}  {}",
            lesson.id, lesson.confidence, lesson.status
        )?;
        writeln!(output, "  Trigger: {trigger}")?;
        writeln!(output, "  Hint: {hint}")?;
    }
    Ok(())
}

fn lesson(id: Option<&String>, output: &mut dyn Write) -> CliResult<()> {
    let id = id.ok_or_else(|| CliError::Usage("usage: reflex lesson <id>".to_string()))?;
    let store = Store::open_default()?;
    let lesson = store
        .get_lesson(id)?
        .ok_or_else(|| CliError::Usage(format!("lesson not found: {id}")))?;
    serde_json::to_writer_pretty(output, &json!(lesson))?;
    Ok(())
}

fn cases(output: &mut dyn Write) -> CliResult<()> {
    let store = Store::open_default()?;
    writeln!(output, "Reflex cases:")?;
    for case in store.list_cases(Some("any"), 100)? {
        writeln!(
            output,
            "{}  {}  attempts={}",
            case.id,
            case.status.as_str(),
            case.attempt_ids.len()
        )?;
    }
    Ok(())
}

fn case(id: Option<&String>, output: &mut dyn Write) -> CliResult<()> {
    let id = id.ok_or_else(|| CliError::Usage("usage: reflex case <id>".to_string()))?;
    let store = Store::open_default()?;
    let case = store
        .get_episode(id)?
        .ok_or_else(|| CliError::Usage(format!("case not found: {id}")))?;
    serde_json::to_writer_pretty(output, &json!(case))?;
    Ok(())
}

fn update_lesson(id: Option<&String>, status: &str, output: &mut dyn Write) -> CliResult<()> {
    let id = id.ok_or_else(|| CliError::Usage(format!("usage: reflex {status} <lesson-id>")))?;
    let store = Store::open_default()?;
    match status {
        "active" => store.mark_lesson_result(id, "confirmed")?,
        "candidate" => store.update_lesson_status(id, status)?,
        "ignored" => store.update_lesson_status(id, status)?,
        _ => store.update_lesson_status(id, status)?,
    }
    writeln!(output, "{id}: {status}")?;
    Ok(())
}

fn analyze(output: &mut dyn Write) -> CliResult<()> {
    writeln!(
        output,
        "Reflex analyzer is disabled by default; candidate_repaired cases are ready for explicit registration."
    )?;
    Ok(())
}

fn doctor(output: &mut dyn Write) -> CliResult<()> {
    let store = Store::open_default()?;
    let stats = store.stats()?;
    writeln!(output, "Reflex doctor")?;
    writeln!(output, "data_dir={}", store.root().display())?;
    writeln!(output, "storage_bytes={}", stats.db_bytes)?;
    writeln!(output, "attempts={}", stats.attempts)?;
    writeln!(output, "plugin_manifest=.codex-plugin/plugin.json")?;
    writeln!(output, "hooks=hooks/hooks.json")?;
    writeln!(output, "mcp=.mcp.json")?;
    writeln!(
        output,
        "Remember: Codex bundled hooks must be reviewed and trusted before they run."
    )?;
    Ok(())
}

fn export(output: &mut dyn Write) -> CliResult<()> {
    let store = Store::open_default()?;
    let value = json!({
        "lessons": store.list_lessons(1000)?,
        "cases": store.list_cases(Some("any"), 1000)?,
    });
    serde_json::to_writer_pretty(output, &value)?;
    Ok(())
}

fn purge(args: &[String], output: &mut dyn Write) -> CliResult<()> {
    let keep_recent = parse_keep_recent(args)?;
    let store = Store::open_default()?;
    let report = store.purge_keep_recent(keep_recent)?;
    writeln!(output, "Purged Reflex storage")?;
    writeln!(output, "attempts_deleted={}", report.attempts_deleted)?;
    writeln!(output, "cases_deleted={}", report.cases_deleted)?;
    writeln!(output, "injections_deleted={}", report.injections_deleted)?;
    writeln!(output, "db_bytes_before={}", report.db_bytes_before)?;
    writeln!(output, "db_bytes_after={}", report.db_bytes_after)?;
    Ok(())
}

fn register_demo_lesson(output: &mut dyn Write) -> CliResult<()> {
    let store = Store::open_default()?;
    let result = mcp::register_repair_episode(
        &store,
        RegisterRepairEpisodeInput {
            case_id: None,
            reusable: true,
            failure_summary: "Demo command failed from the wrong working directory.".to_string(),
            repair_summary: "The command passed from services/api with uv run.".to_string(),
            lesson_hint: "API tests in this repo previously passed from `services/api` using `uv run pytest`.".to_string(),
            trigger_description: "Running Python API tests in this repository.".to_string(),
            avoid_when: vec!["frontend tests".to_string()],
            predicates: domain::LessonPredicates {
                tool_family: Some("Bash".to_string()),
                command_family: Some("pytest".to_string()),
                match_executables: vec!["pytest".to_string(), "uv".to_string()],
                ..Default::default()
            },
            scope: "project".to_string(),
            risk_level: "low".to_string(),
            confidence: 0.72,
        },
    )?;
    serde_json::to_writer_pretty(output, &result)?;
    Ok(())
}

fn usage() -> String {
    "usage: reflex <status|lessons|lesson|cases|case|analyze|ignore|promote|demote|doctor|export|purge|hook>".to_string()
}

fn parse_keep_recent(args: &[String]) -> CliResult<usize> {
    if args.is_empty() {
        return Ok(10_000);
    }
    if args.len() == 2 && args[0] == "--keep-recent" {
        return args[1]
            .parse::<usize>()
            .map_err(|_| CliError::Usage("usage: reflex purge [--keep-recent N]".to_string()));
    }
    if args.len() == 2 && args[0] == "--older-than" {
        return Err(CliError::Usage(
            "unsupported purge option: use --keep-recent N".to_string(),
        ));
    }
    Err(CliError::Usage(
        "usage: reflex purge [--keep-recent N]".to_string(),
    ))
}
