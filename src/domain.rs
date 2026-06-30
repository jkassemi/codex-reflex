use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum AttemptResult {
    Success,
    Failure,
    Unknown,
}

impl AttemptResult {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Success => "success",
            Self::Failure => "failure",
            Self::Unknown => "unknown",
        }
    }

    pub fn parse(value: &str) -> Self {
        match value {
            "success" => Self::Success,
            "failure" => Self::Failure,
            _ => Self::Unknown,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolAttempt {
    pub id: String,
    pub session_id: String,
    pub turn_id: Option<String>,
    pub tool_use_id: Option<String>,
    pub ts: String,
    pub cwd: String,
    pub project_hash: String,
    pub tool_name: String,
    pub tool_input_json: Value,
    pub tool_response_summary_json: Value,
    pub result: AttemptResult,
    pub failure_kind: Option<String>,
    pub raw_event_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum EpisodeStatus {
    Open,
    Repairing,
    CandidateRepaired,
    ModelRegistered,
    Analyzed,
    CandidateLesson,
    ActiveLesson,
    Ignored,
    Retired,
    NotReusable,
}

impl EpisodeStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Repairing => "repairing",
            Self::CandidateRepaired => "candidate_repaired",
            Self::ModelRegistered => "model_registered",
            Self::Analyzed => "analyzed",
            Self::CandidateLesson => "candidate_lesson",
            Self::ActiveLesson => "active_lesson",
            Self::Ignored => "ignored",
            Self::Retired => "retired",
            Self::NotReusable => "not_reusable",
        }
    }

    pub fn parse(value: &str) -> Self {
        match value {
            "repairing" => Self::Repairing,
            "candidate_repaired" => Self::CandidateRepaired,
            "model_registered" => Self::ModelRegistered,
            "analyzed" => Self::Analyzed,
            "candidate_lesson" => Self::CandidateLesson,
            "active_lesson" => Self::ActiveLesson,
            "ignored" => Self::Ignored,
            "retired" => Self::Retired,
            "not_reusable" => Self::NotReusable,
            _ => Self::Open,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepairEpisode {
    pub id: String,
    pub session_id: String,
    pub turn_id: Option<String>,
    pub project_hash: String,
    pub status: EpisodeStatus,
    pub user_intent_excerpt: Option<String>,
    pub opened_by_attempt_id: String,
    pub attempt_ids: Vec<String>,
    pub created_at: String,
    pub updated_at: String,
    pub expires_at: String,
    pub resolution_json: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Lesson {
    pub id: String,
    pub project_hash: String,
    pub status: String,
    pub scope_json: Value,
    pub trigger_json: Value,
    pub lesson_json: Value,
    pub evidence_json: Value,
    pub confidence: f64,
    pub times_injected: i64,
    pub times_confirmed: i64,
    pub times_contradicted: i64,
    pub created_at: String,
    pub updated_at: String,
    pub last_injected_at: Option<String>,
    pub last_confirmed_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingTool {
    pub session_id: String,
    pub turn_id: Option<String>,
    pub cwd: String,
    pub project_hash: String,
    pub tool_name: String,
    pub tool_input_json: Value,
    pub summary: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct LessonPredicates {
    pub tool_family: Option<String>,
    pub command_family: Option<String>,
    #[serde(default)]
    pub match_executables: Vec<String>,
    #[serde(default)]
    pub required_env: BTreeMap<String, String>,
    pub required_cwd: Option<String>,
    pub preferred_command: Option<String>,
    #[serde(default)]
    pub suppress_when: PredicateConditions,
    #[serde(default)]
    pub block_when: PredicateConditions,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct PredicateConditions {
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    #[serde(default)]
    pub command_contains: Vec<String>,
    pub executable: Option<String>,
}

pub fn new_id(prefix: &str) -> String {
    let seq = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let pid = std::process::id();
    let thread = format!("{:?}", std::thread::current().id());
    let mut hasher = Sha256::new();
    hasher.update(now.to_le_bytes());
    hasher.update(pid.to_le_bytes());
    hasher.update(seq.to_le_bytes());
    hasher.update(thread.as_bytes());
    let digest = hasher.finalize();
    format!(
        "{prefix}_{:x}_{pid:x}_{seq:x}_{:02x}{:02x}{:02x}{:02x}",
        now, digest[0], digest[1], digest[2], digest[3]
    )
}

pub fn project_hash(cwd: &str) -> String {
    let path = Path::new(cwd);
    let canonical = project_root(path)
        .canonicalize()
        .unwrap_or_else(|_| path.to_path_buf());
    let mut hasher = Sha256::new();
    hasher.update(canonical.to_string_lossy().as_bytes());
    format!("sha256:{:x}", hasher.finalize())
}

fn project_root(path: &Path) -> std::path::PathBuf {
    let mut current = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    if current.is_file() {
        current.pop();
    }
    loop {
        if current.join(".git").exists()
            || current.join("Cargo.toml").exists()
            || current.join("package.json").exists()
            || current.join("pyproject.toml").exists()
        {
            return current;
        }
        if !current.pop() {
            return path.to_path_buf();
        }
    }
}

pub fn short_string(value: &Value, max: usize) -> String {
    let mut text = match value {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    };
    text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if text.len() > max {
        text.truncate(max);
        text.push_str("...");
    }
    text
}
