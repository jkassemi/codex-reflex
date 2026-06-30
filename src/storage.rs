use crate::domain::{AttemptResult, EpisodeStatus, Lesson, RepairEpisode, ToolAttempt};
use crate::time;
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{json, Value};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy)]
pub struct StorageStats {
    pub attempts: i64,
    pub cases: i64,
    pub lessons: i64,
    pub injections: i64,
    pub db_bytes: u64,
}

#[derive(Debug, Clone, Copy)]
pub struct PurgeReport {
    pub attempts_deleted: usize,
    pub cases_deleted: usize,
    pub injections_deleted: usize,
    pub db_bytes_before: u64,
    pub db_bytes_after: u64,
}

#[derive(Debug)]
pub enum StorageError {
    Io(std::io::Error),
    Sql(rusqlite::Error),
    Json(serde_json::Error),
}

impl std::fmt::Display for StorageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(err) => write!(f, "io error: {err}"),
            Self::Sql(err) => write!(f, "sqlite error: {err}"),
            Self::Json(err) => write!(f, "json error: {err}"),
        }
    }
}

impl std::error::Error for StorageError {}

impl From<std::io::Error> for StorageError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<rusqlite::Error> for StorageError {
    fn from(value: rusqlite::Error) -> Self {
        Self::Sql(value)
    }
}

impl From<serde_json::Error> for StorageError {
    fn from(value: serde_json::Error) -> Self {
        Self::Json(value)
    }
}

pub type StorageResult<T> = Result<T, StorageError>;

pub struct Store {
    conn: Connection,
    root: PathBuf,
}

impl Store {
    pub fn open_default() -> StorageResult<Self> {
        let primary = data_dir();
        match Self::open(&primary) {
            Ok(store) => Ok(store),
            Err(_) if env::var("PLUGIN_DATA").is_err() && env::var("REFLEX_DATA").is_err() => {
                Self::open(env::temp_dir().join("reflex"))
            }
            Err(err) => Err(err),
        }
    }

    pub fn open(root: impl AsRef<Path>) -> StorageResult<Self> {
        fs::create_dir_all(root.as_ref())?;
        let db_path = root.as_ref().join("reflex.db");
        let conn = Connection::open(db_path)?;
        let store = Self {
            conn,
            root: root.as_ref().to_path_buf(),
        };
        store.configure_connection()?;
        store.migrate()?;
        Ok(store)
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn migrate(&self) -> StorageResult<()> {
        self.conn
            .execute_batch(include_str!("../migrations/001_init.sql"))?;
        Ok(())
    }

    fn configure_connection(&self) -> StorageResult<()> {
        self.conn.execute_batch(
            "pragma journal_mode = WAL;
             pragma synchronous = NORMAL;
             pragma busy_timeout = 2500;
             pragma foreign_keys = ON;",
        )?;
        Ok(())
    }

    pub fn insert_attempt(&self, attempt: &ToolAttempt) -> StorageResult<()> {
        self.conn.execute(
            "insert into tool_attempts (
                id, session_id, turn_id, tool_use_id, ts, cwd, project_hash, tool_name,
                tool_input_json, tool_response_summary_json, result, failure_kind, raw_event_path
            ) values (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
            params![
                attempt.id,
                attempt.session_id,
                attempt.turn_id,
                attempt.tool_use_id,
                attempt.ts,
                attempt.cwd,
                attempt.project_hash,
                attempt.tool_name,
                serde_json::to_string(&attempt.tool_input_json)?,
                serde_json::to_string(&attempt.tool_response_summary_json)?,
                attempt.result.as_str(),
                attempt.failure_kind,
                attempt.raw_event_path,
            ],
        )?;
        Ok(())
    }

    pub fn get_attempt(&self, id: &str) -> StorageResult<Option<ToolAttempt>> {
        self.conn
            .query_row(
                "select id, session_id, turn_id, tool_use_id, ts, cwd, project_hash, tool_name,
                    tool_input_json, tool_response_summary_json, result, failure_kind, raw_event_path
                 from tool_attempts where id = ?1",
                params![id],
                row_to_attempt,
            )
            .optional()
            .map_err(StorageError::from)
    }

    pub fn list_attempts(&self, limit: usize) -> StorageResult<Vec<ToolAttempt>> {
        let mut stmt = self.conn.prepare(
            "select id, session_id, turn_id, tool_use_id, ts, cwd, project_hash, tool_name,
                tool_input_json, tool_response_summary_json, result, failure_kind, raw_event_path
             from tool_attempts order by ts desc limit ?1",
        )?;
        let rows = stmt.query_map(params![limit as i64], row_to_attempt)?;
        collect_rows(rows)
    }

    pub fn insert_episode(&self, episode: &RepairEpisode) -> StorageResult<()> {
        self.conn.execute(
            "insert into repair_episodes (
                id, session_id, turn_id, project_hash, status, user_intent_excerpt,
                opened_by_attempt_id, attempt_ids_json, created_at, updated_at, expires_at, resolution_json
            ) values (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            params![
                episode.id,
                episode.session_id,
                episode.turn_id,
                episode.project_hash,
                episode.status.as_str(),
                episode.user_intent_excerpt,
                episode.opened_by_attempt_id,
                serde_json::to_string(&episode.attempt_ids)?,
                episode.created_at,
                episode.updated_at,
                episode.expires_at,
                optional_json_text(&episode.resolution_json)?,
            ],
        )?;
        Ok(())
    }

    pub fn update_episode(&self, episode: &RepairEpisode) -> StorageResult<()> {
        self.conn.execute(
            "update repair_episodes
             set status = ?2, attempt_ids_json = ?3, updated_at = ?4, resolution_json = ?5
             where id = ?1",
            params![
                episode.id,
                episode.status.as_str(),
                serde_json::to_string(&episode.attempt_ids)?,
                episode.updated_at,
                optional_json_text(&episode.resolution_json)?,
            ],
        )?;
        Ok(())
    }

    pub fn find_open_episode(
        &self,
        session_id: &str,
        project_hash: &str,
    ) -> StorageResult<Option<RepairEpisode>> {
        self.conn
            .query_row(
                "select id, session_id, turn_id, project_hash, status, user_intent_excerpt,
                    opened_by_attempt_id, attempt_ids_json, created_at, updated_at, expires_at, resolution_json
                 from repair_episodes
                 where session_id = ?1 and project_hash = ?2
                   and status in ('open', 'repairing', 'candidate_repaired')
                 order by created_at desc limit 1",
                params![session_id, project_hash],
                row_to_episode,
            )
            .optional()
            .map_err(StorageError::from)
    }

    pub fn get_episode(&self, id: &str) -> StorageResult<Option<RepairEpisode>> {
        self.conn
            .query_row(
                "select id, session_id, turn_id, project_hash, status, user_intent_excerpt,
                    opened_by_attempt_id, attempt_ids_json, created_at, updated_at, expires_at, resolution_json
                 from repair_episodes where id = ?1",
                params![id],
                row_to_episode,
            )
            .optional()
            .map_err(StorageError::from)
    }

    pub fn list_cases(
        &self,
        status: Option<&str>,
        limit: usize,
    ) -> StorageResult<Vec<RepairEpisode>> {
        let sql_any = "select id, session_id, turn_id, project_hash, status, user_intent_excerpt,
            opened_by_attempt_id, attempt_ids_json, created_at, updated_at, expires_at, resolution_json
            from repair_episodes order by updated_at desc limit ?1";
        let sql_status = "select id, session_id, turn_id, project_hash, status, user_intent_excerpt,
            opened_by_attempt_id, attempt_ids_json, created_at, updated_at, expires_at, resolution_json
            from repair_episodes where status = ?1 order by updated_at desc limit ?2";
        if let Some(status) = status.filter(|s| *s != "any") {
            let mut stmt = self.conn.prepare(sql_status)?;
            let rows = stmt.query_map(params![status, limit as i64], row_to_episode)?;
            collect_rows(rows)
        } else {
            let mut stmt = self.conn.prepare(sql_any)?;
            let rows = stmt.query_map(params![limit as i64], row_to_episode)?;
            collect_rows(rows)
        }
    }

    pub fn insert_lesson(&self, lesson: &Lesson) -> StorageResult<()> {
        self.conn.execute(
            "insert into lessons (
                id, project_hash, status, scope_json, trigger_json, lesson_json, evidence_json,
                confidence, times_injected, times_confirmed, times_contradicted,
                created_at, updated_at, last_injected_at, last_confirmed_at
            ) values (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
            params![
                lesson.id,
                lesson.project_hash,
                lesson.status,
                serde_json::to_string(&lesson.scope_json)?,
                serde_json::to_string(&lesson.trigger_json)?,
                serde_json::to_string(&lesson.lesson_json)?,
                serde_json::to_string(&lesson.evidence_json)?,
                lesson.confidence,
                lesson.times_injected,
                lesson.times_confirmed,
                lesson.times_contradicted,
                lesson.created_at,
                lesson.updated_at,
                lesson.last_injected_at,
                lesson.last_confirmed_at,
            ],
        )?;
        Ok(())
    }

    pub fn get_lesson(&self, id: &str) -> StorageResult<Option<Lesson>> {
        self.conn
            .query_row(
                "select id, project_hash, status, scope_json, trigger_json, lesson_json, evidence_json,
                    confidence, times_injected, times_confirmed, times_contradicted,
                    created_at, updated_at, last_injected_at, last_confirmed_at
                 from lessons where id = ?1",
                params![id],
                row_to_lesson,
            )
            .optional()
            .map_err(StorageError::from)
    }

    pub fn list_lessons(&self, limit: usize) -> StorageResult<Vec<Lesson>> {
        let mut stmt = self.conn.prepare(
            "select id, project_hash, status, scope_json, trigger_json, lesson_json, evidence_json,
                confidence, times_injected, times_confirmed, times_contradicted,
                created_at, updated_at, last_injected_at, last_confirmed_at
             from lessons where status not in ('ignored', 'retired') order by updated_at desc limit ?1",
        )?;
        let rows = stmt.query_map(params![limit as i64], row_to_lesson)?;
        collect_rows(rows)
    }

    pub fn lessons_for_project(&self, project_hash: &str) -> StorageResult<Vec<Lesson>> {
        let mut stmt = self.conn.prepare(
            "select id, project_hash, status, scope_json, trigger_json, lesson_json, evidence_json,
                confidence, times_injected, times_confirmed, times_contradicted,
                created_at, updated_at, last_injected_at, last_confirmed_at
             from lessons
             where project_hash = ?1 and status in ('candidate', 'active')
             order by confidence desc, updated_at desc",
        )?;
        let rows = stmt.query_map(params![project_hash], row_to_lesson)?;
        collect_rows(rows)
    }

    pub fn update_lesson_status(&self, id: &str, status: &str) -> StorageResult<()> {
        self.conn.execute(
            "update lessons set status = ?2, updated_at = ?3 where id = ?1",
            params![id, status, time::now_text()],
        )?;
        Ok(())
    }

    pub fn mark_lesson_result(&self, id: &str, result: &str) -> StorageResult<()> {
        let now = time::now_text();
        match result {
            "confirmed" => {
                self.conn.execute(
                    "update lessons
                     set times_confirmed = times_confirmed + 1,
                         confidence = min(1.0, confidence + 0.12),
                         status = case when confidence + 0.12 >= 0.78 and times_confirmed + 1 >= 2 then 'active' else status end,
                         last_confirmed_at = ?2,
                         updated_at = ?2
                     where id = ?1",
                    params![id, now],
                )?;
            }
            "contradicted" => {
                self.conn.execute(
                    "update lessons
                     set times_contradicted = times_contradicted + 1,
                         confidence = max(0.0, confidence - 0.25),
                         status = case when times_contradicted + 1 >= 3 then 'retired' else 'candidate' end,
                         updated_at = ?2
                     where id = ?1",
                    params![id, now],
                )?;
            }
            _ => {
                self.conn.execute(
                    "update lessons set updated_at = ?2 where id = ?1",
                    params![id, now],
                )?;
            }
        }
        Ok(())
    }

    pub fn record_injection(
        &self,
        lesson_id: &str,
        attempt_id: Option<&str>,
        session_id: &str,
        pending_tool_summary: &Value,
    ) -> StorageResult<()> {
        let now = time::now_text();
        self.conn.execute(
            "insert into lesson_injections (
                id, lesson_id, attempt_id, session_id, ts, pending_tool_summary_json, subsequent_result
            ) values (?1, ?2, ?3, ?4, ?5, ?6, null)",
            params![
                crate::domain::new_id("inj"),
                lesson_id,
                attempt_id,
                session_id,
                now,
                serde_json::to_string(pending_tool_summary)?,
            ],
        )?;
        self.conn.execute(
            "update lessons set times_injected = times_injected + 1, last_injected_at = ?2, updated_at = ?2 where id = ?1",
            params![lesson_id, now],
        )?;
        Ok(())
    }

    pub fn stats(&self) -> StorageResult<StorageStats> {
        Ok(StorageStats {
            attempts: self.count("tool_attempts")?,
            cases: self.count("repair_episodes")?,
            lessons: self.count("lessons")?,
            injections: self.count("lesson_injections")?,
            db_bytes: self.db_bytes(),
        })
    }

    pub fn purge_keep_recent(&self, keep_recent_attempts: usize) -> StorageResult<PurgeReport> {
        let before = self.db_bytes();
        let attempts_deleted = self.conn.execute(
            "delete from tool_attempts
             where id not in (
               select id from tool_attempts order by ts desc limit ?1
             )",
            params![keep_recent_attempts as i64],
        )?;
        let cases_deleted = self.conn.execute(
            "delete from repair_episodes
             where opened_by_attempt_id not in (select id from tool_attempts)
               and status not in ('model_registered', 'candidate_lesson', 'active_lesson')",
            [],
        )?;
        let injections_deleted = self.conn.execute(
            "delete from lesson_injections
             where lesson_id not in (select id from lessons)
                or attempt_id is not null and attempt_id not in (select id from tool_attempts)",
            [],
        )?;
        self.vacuum()?;
        Ok(PurgeReport {
            attempts_deleted,
            cases_deleted,
            injections_deleted,
            db_bytes_before: before,
            db_bytes_after: self.db_bytes(),
        })
    }

    pub fn vacuum(&self) -> StorageResult<()> {
        self.conn.execute_batch(
            "pragma wal_checkpoint(TRUNCATE); vacuum; pragma wal_checkpoint(TRUNCATE);",
        )?;
        Ok(())
    }

    fn count(&self, table: &str) -> StorageResult<i64> {
        let sql = format!("select count(*) from {table}");
        self.conn
            .query_row(&sql, [], |row| row.get(0))
            .map_err(StorageError::from)
    }

    fn db_bytes(&self) -> u64 {
        let mut total = 0;
        for suffix in ["reflex.db", "reflex.db-wal", "reflex.db-shm"] {
            let path = self.root.join(suffix);
            total += fs::metadata(path).map(|meta| meta.len()).unwrap_or(0);
        }
        total
    }
}

pub fn data_dir() -> PathBuf {
    if let Ok(path) = env::var("PLUGIN_DATA") {
        return PathBuf::from(path);
    }
    if let Ok(path) = env::var("REFLEX_DATA") {
        return PathBuf::from(path);
    }
    if let Ok(home) = env::var("HOME") {
        return PathBuf::from(home).join(".local/share/reflex");
    }
    PathBuf::from(".reflex")
}

fn row_to_attempt(row: &rusqlite::Row<'_>) -> rusqlite::Result<ToolAttempt> {
    let input: String = row.get(8)?;
    let response: String = row.get(9)?;
    let result: String = row.get(10)?;
    Ok(ToolAttempt {
        id: row.get(0)?,
        session_id: row.get(1)?,
        turn_id: row.get(2)?,
        tool_use_id: row.get(3)?,
        ts: row.get(4)?,
        cwd: row.get(5)?,
        project_hash: row.get(6)?,
        tool_name: row.get(7)?,
        tool_input_json: serde_json::from_str(&input).unwrap_or_else(|_| json!({})),
        tool_response_summary_json: serde_json::from_str(&response).unwrap_or_else(|_| json!({})),
        result: AttemptResult::parse(&result),
        failure_kind: row.get(11)?,
        raw_event_path: row.get(12)?,
    })
}

fn row_to_episode(row: &rusqlite::Row<'_>) -> rusqlite::Result<RepairEpisode> {
    let status: String = row.get(4)?;
    let attempts: String = row.get(7)?;
    let resolution: Option<String> = row.get(11)?;
    Ok(RepairEpisode {
        id: row.get(0)?,
        session_id: row.get(1)?,
        turn_id: row.get(2)?,
        project_hash: row.get(3)?,
        status: EpisodeStatus::parse(&status),
        user_intent_excerpt: row.get(5)?,
        opened_by_attempt_id: row.get(6)?,
        attempt_ids: serde_json::from_str(&attempts).unwrap_or_default(),
        created_at: row.get(8)?,
        updated_at: row.get(9)?,
        expires_at: row.get(10)?,
        resolution_json: resolution.and_then(|text| serde_json::from_str(&text).ok()),
    })
}

fn row_to_lesson(row: &rusqlite::Row<'_>) -> rusqlite::Result<Lesson> {
    let scope: String = row.get(3)?;
    let trigger: String = row.get(4)?;
    let lesson: String = row.get(5)?;
    let evidence: String = row.get(6)?;
    Ok(Lesson {
        id: row.get(0)?,
        project_hash: row.get(1)?,
        status: row.get(2)?,
        scope_json: serde_json::from_str(&scope).unwrap_or_else(|_| json!({})),
        trigger_json: serde_json::from_str(&trigger).unwrap_or_else(|_| json!({})),
        lesson_json: serde_json::from_str(&lesson).unwrap_or_else(|_| json!({})),
        evidence_json: serde_json::from_str(&evidence).unwrap_or_else(|_| json!({})),
        confidence: row.get(7)?,
        times_injected: row.get(8)?,
        times_confirmed: row.get(9)?,
        times_contradicted: row.get(10)?,
        created_at: row.get(11)?,
        updated_at: row.get(12)?,
        last_injected_at: row.get(13)?,
        last_confirmed_at: row.get(14)?,
    })
}

fn optional_json_text(value: &Option<Value>) -> StorageResult<Option<String>> {
    value
        .as_ref()
        .map(serde_json::to_string)
        .transpose()
        .map_err(StorageError::from)
}

fn collect_rows<T, F>(rows: rusqlite::MappedRows<'_, F>) -> StorageResult<Vec<T>>
where
    F: FnMut(&rusqlite::Row<'_>) -> rusqlite::Result<T>,
{
    let mut values = Vec::new();
    for row in rows {
        values.push(row?);
    }
    Ok(values)
}
