create table if not exists tool_attempts (
  id text primary key,
  session_id text not null,
  turn_id text,
  tool_use_id text,
  ts text not null,
  cwd text not null,
  project_hash text not null,
  tool_name text not null,
  tool_input_json text not null,
  tool_response_summary_json text not null,
  result text not null,
  failure_kind text,
  raw_event_path text
);

create table if not exists repair_episodes (
  id text primary key,
  session_id text not null,
  turn_id text,
  project_hash text not null,
  status text not null,
  user_intent_excerpt text,
  opened_by_attempt_id text not null,
  attempt_ids_json text not null,
  created_at text not null,
  updated_at text not null,
  expires_at text not null,
  resolution_json text
);

create table if not exists lessons (
  id text primary key,
  project_hash text not null,
  status text not null,
  scope_json text not null,
  trigger_json text not null,
  lesson_json text not null,
  evidence_json text not null,
  confidence real not null,
  times_injected integer not null default 0,
  times_confirmed integer not null default 0,
  times_contradicted integer not null default 0,
  created_at text not null,
  updated_at text not null,
  last_injected_at text,
  last_confirmed_at text
);

create table if not exists lesson_injections (
  id text primary key,
  lesson_id text not null,
  attempt_id text,
  session_id text not null,
  ts text not null,
  pending_tool_summary_json text not null,
  subsequent_result text
);

create index if not exists idx_tool_attempts_session_project_ts
  on tool_attempts(session_id, project_hash, ts);

create index if not exists idx_tool_attempts_project_ts
  on tool_attempts(project_hash, ts);

create index if not exists idx_repair_episodes_session_project_status
  on repair_episodes(session_id, project_hash, status, updated_at);

create index if not exists idx_repair_episodes_status_updated
  on repair_episodes(status, updated_at);

create index if not exists idx_lessons_project_status_confidence
  on lessons(project_hash, status, confidence, updated_at);

create index if not exists idx_lesson_injections_lesson_ts
  on lesson_injections(lesson_id, ts);
