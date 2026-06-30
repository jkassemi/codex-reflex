use crate::domain::{Lesson, LessonPredicates, PendingTool, PredicateConditions};
use crate::storage::{StorageResult, Store};
use serde_json::{json, Value};
use std::collections::BTreeSet;

#[derive(Debug, Clone)]
pub struct MatchedLesson {
    pub lesson: Lesson,
    pub score: f64,
}

pub fn find_matches(
    store: &Store,
    pending: &PendingTool,
    limit: usize,
) -> StorageResult<Vec<MatchedLesson>> {
    let mut matches = Vec::new();
    for lesson in store.lessons_for_project(&pending.project_hash)? {
        if !risk_policy_allows(&lesson) || anti_trigger_matches(&lesson, pending) {
            continue;
        }
        let Some(score) = match_score(&lesson, pending) else {
            continue;
        };
        let threshold = threshold(&lesson);
        if score >= threshold {
            matches.push(MatchedLesson { lesson, score });
        }
    }
    matches.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| {
                b.lesson
                    .confidence
                    .partial_cmp(&a.lesson.confidence)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
    });
    let mut seen = BTreeSet::new();
    matches.retain(|matched| seen.insert(lesson_signature(&matched.lesson)));
    matches.truncate(limit);
    Ok(matches)
}

pub fn injection_context(matches: &[MatchedLesson]) -> String {
    let mut lines = Vec::new();
    let mut total = 0usize;
    for matched in matches.iter().take(2) {
        let hint = text_field(&matched.lesson.lesson_json, "hint")
            .or_else(|| text_field(&matched.lesson.lesson_json, "lesson_hint"))
            .unwrap_or_else(|| "Similar tool calls have a stored Reflex lesson.".to_string());
        let guidance = lesson_predicates(&matched.lesson)
            .and_then(|predicates| predicates.preferred_command)
            .map(|command| {
                format!(
                    "Use `{}` for this command. {}",
                    truncate(&command, 160),
                    hint
                )
            })
            .unwrap_or(hint);
        let avoid = avoid_text(&matched.lesson.trigger_json);
        let mut line = if avoid.is_empty() {
            format!("Reflex: {}", truncate(&guidance, 260))
        } else {
            format!(
                "Reflex: {} Avoid if {}.",
                truncate(&guidance, 220),
                truncate(&avoid, 160)
            )
        };
        if line.len() > 300 {
            line.truncate(300);
        }
        if total + line.len() > 600 {
            break;
        }
        total += line.len();
        lines.push(line);
    }
    lines.join("\n")
}

pub fn block_reason(matches: &[MatchedLesson]) -> Option<String> {
    matches.iter().find_map(|matched| {
        let predicates = lesson_predicates(&matched.lesson)?;
        let preferred_command = predicates.preferred_command?;
        if predicates.required_env.is_empty() {
            return None;
        }
        Some(format!(
            "Reflex blocked this command because it violates a stored operational repair. Run `{}` instead. Do not call register_repair_episode for this same Reflex-provided repair.",
            truncate(&preferred_command, 180)
        ))
    })
}

pub fn pending_summary(tool_name: &str, tool_input_json: &serde_json::Value, cwd: &str) -> String {
    let command = command_text(tool_input_json);
    format!("{tool_name} cwd={cwd} {command}")
}

pub fn pending_summary_json(pending: &PendingTool) -> serde_json::Value {
    json!({
        "tool_name": pending.tool_name,
        "cwd": pending.cwd,
        "summary": pending.summary,
    })
}

fn match_score(lesson: &Lesson, pending: &PendingTool) -> Option<f64> {
    let predicates = lesson_predicates(lesson)?;
    if structured_matches(&predicates, pending) && !structured_satisfied(&predicates, pending) {
        return Some(1.0);
    }
    None
}

fn threshold(lesson: &Lesson) -> f64 {
    let base = if lesson.status == "active" {
        0.55
    } else {
        0.62
    };
    let risk = text_field(&lesson.lesson_json, "risk_level").unwrap_or_default();
    if risk == "high" {
        base + 0.15
    } else {
        base
    }
}

fn risk_policy_allows(lesson: &Lesson) -> bool {
    text_field(&lesson.lesson_json, "rewrite_allowed").as_deref() != Some("true")
}

fn structured_matches(predicates: &LessonPredicates, pending: &PendingTool) -> bool {
    if let Some(tool_family) = predicates.tool_family.as_deref() {
        if !pending.tool_name.eq_ignore_ascii_case(tool_family) {
            return false;
        }
    }

    if let Some(required_cwd) = predicates.required_cwd.as_deref() {
        if pending.cwd != required_cwd && !pending.cwd.starts_with(&format!("{required_cwd}/")) {
            return false;
        }
    }

    let command = command_text(&pending.tool_input_json);
    let profile = CommandProfile::from_command(&command);
    let family_matches = predicates
        .command_family
        .as_deref()
        .map(|command_family| profile.matches_family(command_family))
        .unwrap_or(false);
    let executable_matches = predicates
        .match_executables
        .iter()
        .any(|executable| profile.matches_executable(executable));

    family_matches || executable_matches
}

fn structured_satisfied(predicates: &LessonPredicates, pending: &PendingTool) -> bool {
    let command = command_text(&pending.tool_input_json);
    let profile = CommandProfile::from_command(&command);
    if !predicates.required_env.is_empty() {
        return env_satisfied(&profile, &predicates.required_env);
    }
    conditions_match(&predicates.suppress_when, &profile, &command)
}

fn anti_trigger_matches(lesson: &Lesson, pending: &PendingTool) -> bool {
    let pending_text = format!(
        "{} {}",
        pending.summary,
        command_text(&pending.tool_input_json)
    )
    .to_ascii_lowercase();
    lesson
        .trigger_json
        .get("negative_terms")
        .and_then(|v| v.as_array())
        .map(|terms| {
            terms.iter().any(|term| {
                term.as_str()
                    .map(|text| pending_text.contains(&text.to_ascii_lowercase()))
                    .unwrap_or(false)
            })
        })
        .unwrap_or(false)
}

fn avoid_text(value: &serde_json::Value) -> String {
    value
        .get("avoid_when")
        .and_then(|v| v.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|v| v.as_str())
                .collect::<Vec<_>>()
                .join("; ")
        })
        .unwrap_or_default()
}

fn text_field(value: &serde_json::Value, field: &str) -> Option<String> {
    value.get(field).map(|value| match value {
        serde_json::Value::String(text) => text.clone(),
        serde_json::Value::Bool(value) => value.to_string(),
        serde_json::Value::Number(value) => value.to_string(),
        other => other.to_string(),
    })
}

fn lesson_predicates(lesson: &Lesson) -> Option<LessonPredicates> {
    let value = lesson.trigger_json.get("predicates")?;
    if value.is_null() {
        return None;
    }
    serde_json::from_value(value.clone()).ok()
}

fn lesson_text(lesson: &Lesson) -> String {
    format!(
        "{} {} {}",
        lesson.trigger_json, lesson.lesson_json, lesson.scope_json
    )
}

fn lesson_signature(lesson: &Lesson) -> String {
    let text = lesson_text(lesson);
    let intents = command_intents(&text)
        .into_iter()
        .collect::<Vec<_>>()
        .join(",");
    let env = env_assignments(&text).join(",");
    if !env.is_empty() {
        format!("env:{env}|intent:{intents}")
    } else {
        format!("intent:{intents}")
    }
}

fn command_text(tool_input_json: &Value) -> String {
    tool_input_json
        .get("command")
        .and_then(|v| v.as_str())
        .or_else(|| tool_input_json.get("cmd").and_then(|v| v.as_str()))
        .or_else(|| tool_input_json.get("query").and_then(|v| v.as_str()))
        .map(str::to_string)
        .unwrap_or_else(|| tool_input_json.to_string())
}

fn conditions_match(
    conditions: &PredicateConditions,
    profile: &CommandProfile,
    command: &str,
) -> bool {
    if !conditions.env.is_empty() && env_satisfied(profile, &conditions.env) {
        return true;
    }
    if let Some(executable) = conditions.executable.as_deref() {
        if profile.matches_executable(executable) {
            return true;
        }
    }
    let command_lower = command.to_ascii_lowercase();
    conditions
        .command_contains
        .iter()
        .any(|fragment| command_lower.contains(&fragment.to_ascii_lowercase()))
}

fn env_satisfied(
    profile: &CommandProfile,
    expected: &std::collections::BTreeMap<String, String>,
) -> bool {
    expected.iter().all(|(key, value)| {
        profile
            .env
            .get(key)
            .map(|actual| actual == value)
            .unwrap_or(false)
    })
}

#[derive(Debug)]
struct CommandProfile {
    executable: Option<String>,
    env: std::collections::BTreeMap<String, String>,
    tokens: BTreeSet<String>,
}

impl CommandProfile {
    fn from_command(command: &str) -> Self {
        let mut env = std::collections::BTreeMap::new();
        let mut executable = None;
        let tokens = tokens(command);
        let parts = command.split_whitespace().collect::<Vec<_>>();
        let mut index = 0usize;
        if parts.get(index) == Some(&"env") {
            index += 1;
        }
        while let Some(part) = parts.get(index) {
            let trimmed = trim_shell_token(part);
            if trimmed == "export" || trimmed == "&&" {
                index += 1;
                continue;
            }
            if let Some((key, value)) = trimmed.split_once('=') {
                if is_env_key(key) {
                    env.insert(key.to_string(), value.to_string());
                    index += 1;
                    continue;
                }
            }
            executable = Some(trimmed.to_string());
            break;
        }
        Self {
            executable,
            env,
            tokens,
        }
    }

    fn matches_family(&self, family: &str) -> bool {
        let family = normalize(family);
        self.tokens.contains(&family)
            || self
                .executable
                .as_deref()
                .map(|executable| normalize(executable_basename(executable)) == family)
                .unwrap_or(false)
    }

    fn matches_executable(&self, expected: &str) -> bool {
        let Some(actual) = self.executable.as_deref() else {
            return false;
        };
        actual == expected
            || executable_basename(actual) == executable_basename(expected)
            || normalize(actual) == normalize(expected)
    }
}

fn trim_shell_token(token: &str) -> &str {
    token.trim_matches(|ch: char| {
        matches!(
            ch,
            '`' | '\'' | '"' | ',' | ';' | ':' | ')' | ']' | '}' | '(' | '[' | '{'
        )
    })
}

fn is_env_key(key: &str) -> bool {
    key.chars()
        .all(|ch| ch.is_ascii_uppercase() || ch.is_ascii_digit() || ch == '_')
}

fn executable_basename(executable: &str) -> &str {
    executable.rsplit('/').next().unwrap_or(executable)
}

fn command_intents(text: &str) -> BTreeSet<String> {
    tokens(text)
        .into_iter()
        .filter(|token| COMMAND_INTENTS.contains(&token.as_str()))
        .collect()
}

fn env_assignments(text: &str) -> Vec<String> {
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
    assignments.into_iter().collect()
}

fn tokens(text: &str) -> BTreeSet<String> {
    text.split(|c: char| !c.is_ascii_alphanumeric() && c != '_' && c != '-')
        .map(normalize)
        .filter(|token| token.len() >= 2 && !STOP_WORDS.contains(&token.as_str()))
        .collect()
}

fn normalize(text: &str) -> String {
    text.trim_matches(|c: char| !c.is_ascii_alphanumeric() && c != '_' && c != '-')
        .to_ascii_lowercase()
}

fn truncate(input: &str, max: usize) -> String {
    if input.len() <= max {
        input.to_string()
    } else {
        let mut text = input.to_string();
        text.truncate(max);
        text.push_str("...");
        text
    }
}

const STOP_WORDS: &[&str] = &[
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

const COMMAND_INTENTS: &[&str] = &[
    "aws", "cargo", "docker", "eks", "git", "go", "kubectl", "make", "node", "npm", "pnpm",
    "poetry", "pytest", "python", "test", "uv",
];
