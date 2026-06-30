# Reflex for Codex: full plugin specification

## 1. Product definition

**Name:** `Reflex`
**Repo suggestion:** `reflex-for-codex`
**Plugin package name:** `reflex`
**Binary names:** `reflex`, `reflex-mcp`
**Display name:** `Reflex`
**Tagline:** *Operational repair memory for Codex tool calls.*

**Core promise:**

> Reflex watches failed tool-call episodes, lets Codex register what fixed them, stores a concise scoped lesson, and injects that lesson before similar future tool calls.

**Non-goals for v1:**

```text
No general long-term memory.
No compaction system.
No autonomous cloud sync.
No broad AGENTS.md rewriting.
No automatic production/cloud command rewrites by default.
No secret storage.
No attempt to intercept unsupported Codex tool paths.
```

Codex currently says `PreToolUse` and `PostToolUse` intercept supported Bash, `apply_patch`, and MCP tool calls, but not all shell paths or non-shell/non-MCP tools such as WebSearch, so the README should say “supported Codex tool calls,” not “all tool calls.” ([OpenAI Developers][2])

---

# 2. Central design principle

The plugin should separate **observation**, **interpretation**, and **future application**.

```text
Observation:
  Hooks record what happened.

Interpretation:
  Codex or an analyzer explains what repaired the failure.

Application:
  PreToolUse injects a tiny scoped hint before a similar future tool call.
```

The important thing is that a “repair episode” is not one command. It is a little causal story:

```text
goal / task intent
  → failed attempt(s)
  → diagnostic attempt(s)
  → changed command / changed env / changed cwd / permission / prerequisite
  → success
  → reusable lesson
```

The plugin should store **that** as the learning unit.

---

# 3. When should an episode be recorded?

There are two layers of recording.

## 3.1 Always record minimal tool attempts

Every `PostToolUse` event for supported tools should append a redacted `tool_attempt` row.

This is cheap, local, and not yet “learning.”

```text
PostToolUse
  → redact
  → classify obvious success/failure/unknown
  → append tool_attempt
  → attach to open episode if relevant
```

Codex’s `PostToolUse` receives `tool_name`, `tool_use_id`, `tool_input`, and `tool_response`; for Bash and `apply_patch`, the command is under `tool_input.command`, while MCP tools send their argument object. ([OpenAI Developers][2])

## 3.2 Open a candidate episode only on a meaningful failure

Open an episode when a tool call appears to fail in a way that might produce a reusable operational lesson.

Examples:

```text
non-zero Bash exit
MCP error result
permission denied
approval/escalation request
auth/profile/config failure
wrong cwd
command not found
missing dependency
network/VPN failure
rate limit
tool argument shape error
test/build command failed before code changes
```

Do **not** eagerly create a durable lesson for every test failure. A test failure caused by product code is usually not an operational repair lesson. The analyzer should later classify it as `not_reusable` unless the repair was something like “run from subdir,” “use uv,” “set env var,” or “run generator first.”

## 3.3 Attach later attempts to the open episode

After an episode opens, attach nearby attempts if they are in the same:

```text
session_id
turn_id when available
project_hash
cwd / repo root neighborhood
time window
task-intent window
```

Default windows:

```text
same turn: always eligible
same session: eligible for 30 minutes after failure
cross-turn: eligible if same session and no successful resolution yet
max attempts per episode: 25
max age: 2 hours
```

These should be config values.

## 3.4 Mark an episode “candidate_repaired” after success

After any successful tool call, check open episodes from the same session/project.

Mark as `candidate_repaired` when:

```text
there was at least one prior failure
a later command/tool succeeded
the later success is plausibly related
the agent stops retrying that class of failure
or the model explicitly calls register_repair_episode
```

The plugin should **not** decide the causal lesson with hardcoded command rules. It should just say:

> “This episode may contain a repaired failure. Analyze it.”

## 3.5 Register immediately when Codex recognizes the fix

This is where the model-facing tool comes in.

When Codex has just succeeded after some failures and can tell what changed, it should call:

```text
mcp__reflex__register_repair_episode
```

This should be the primary high-quality path because the main model has the live reasoning context:

```text
"I tried aws eks without a profile, it failed;
then I used --profile platform-admin --region us-east-1 and it worked."
```

Hooks see the facts. The model often understands the causality.

---

# 4. When should episodes be checked?

There are four check points.

## 4.1 `PreToolUse`: check before every supported tool call

This is the most important runtime check.

```text
PreToolUse
  → summarize pending tool call
  → retrieve candidate lessons
  → apply scope/risk/filtering
  → inject at most 1-2 concise hints
```

Example injected context:

```text
Reflex: Similar AWS EKS commands in this repo previously failed without explicit profile/region and later succeeded with `--profile platform-admin --region us-east-1`.
```

Codex supports returning `hookSpecificOutput.additionalContext` from `PreToolUse` so the hint becomes model-visible developer context before the pending tool call runs. ([OpenAI Developers][2])

## 4.2 `PostToolUse`: check after every supported tool call

After each tool result:

```text
PostToolUse
  → record attempt
  → update open episodes
  → if failure, open/extend episode
  → if success after failure, mark candidate_repaired
  → maybe return a tiny “Reflex is tracking this” context line
```

On the first meaningful failure in a turn, `PostToolUse` may return:

```text
Reflex: failure recorded as case case_abc123. If you later find a reusable correction, call `register_repair_episode` with that case id.
```

Do not emit that after every failure. Emit it once per episode or once per turn.

## 4.3 `PermissionRequest`: check approval/escalation patterns

`PermissionRequest` should record approval requests and optionally enforce configured policy.

Default behavior:

```text
record only
defer to normal Codex approval flow
never auto-approve by default
```

Codex’s `PermissionRequest` can allow, deny, or defer to the normal approval prompt; if multiple matching hooks return decisions, any deny wins, while an allow proceeds without surfacing the normal prompt. ([OpenAI Developers][2])

## 4.4 `Stop`: close and analyze unresolved episodes

At `Stop`:

```text
Stop
  → close stale/open episodes
  → enqueue candidate_repaired episodes for analyzer
  → optionally run bounded local analyzer if configured
  → write/update audit files
```

Do not depend on `transcript_path` as the source of truth. Codex provides it, but the docs warn the transcript format is not a stable hook interface. Use your own event log. ([OpenAI Developers][2])

---

# 5. Hook map

Use these hooks in v1:

| Hook                             |    Required? | Purpose                                                                          | Output behavior                                          |
| -------------------------------- | -----------: | -------------------------------------------------------------------------------- | -------------------------------------------------------- |
| `SessionStart`                   |          Yes | Initialize project state and optionally add one-line Reflex availability context | Minimal `additionalContext` only                         |
| `UserPromptSubmit`               | Nice-to-have | Record task intent and user corrections like “that worked” / “use profile X”     | Usually no output                                        |
| `PreToolUse`                     |          Yes | Retrieve relevant lessons before pending tool calls                              | Inject concise `additionalContext`; hint-only by default |
| `PostToolUse`                    |          Yes | Record attempts, open/extend/resolve episodes                                    | Usually silent; sometimes one-line “case recorded”       |
| `PermissionRequest`              |          Yes | Record escalation/network/write approval patterns                                | Defer by default; policy-based deny/allow optional       |
| `Stop`                           |          Yes | Finalize episodes, enqueue/analyze, write audit logs                             | Silent unless error                                      |
| `PreCompact` / `PostCompact`     |    No for v1 | Not needed                                                                       | Cut                                                      |
| `SubagentStart` / `SubagentStop` |        Later | Track spawned agents separately                                                  | v2                                                       |

Hooks are synchronous command handlers today; Codex parses `async` but skips async command hooks, so the hook code must stay fast and bounded. ([OpenAI Developers][2])

---

# 6. MCP tool design

Yes: expose a model-facing MCP server. This is the key to scaling beyond hardcoded command families.

Codex supports MCP servers in CLI and IDE, MCP servers can expose tools/resources/prompts, and plugins can bundle MCP server config so the server is launched from the plugin and controlled through plugin-scoped config. ([OpenAI Developers][3])

## 6.1 MCP server name

```text
reflex
```

## 6.2 MCP server instructions

The first 512 characters should be self-contained because Codex uses MCP server instructions as guidance when deciding how to use the server. ([OpenAI Developers][3])

Suggested MCP server instructions:

```text
Use Reflex after you encounter one or more failed tool calls and later discover a reusable correction. Do not call it for every failure. Call register_repair_episode only after a repair succeeds or the user confirms the fix. Never store secrets, tokens, private keys, session cookies, one-time URLs, or broad rules such as “always use sudo.”
```

## 6.3 Tools

### `register_repair_episode`

Primary model-facing tool.

Purpose:

> Codex calls this after it finds a correction to a prior failed tool call.

Input schema:

```json
{
  "type": "object",
  "additionalProperties": false,
  "required": [
    "case_id",
    "reusable",
    "failure_summary",
    "repair_summary",
    "lesson_hint",
    "trigger_description",
    "avoid_when",
    "scope",
    "risk_level",
    "confidence"
  ],
  "properties": {
    "case_id": {
      "type": ["string", "null"],
      "description": "Reflex case id if provided by hook context; null if unknown."
    },
    "reusable": {
      "type": "boolean",
      "description": "Whether the correction is likely useful in future."
    },
    "failure_summary": {
      "type": "string",
      "maxLength": 800
    },
    "repair_summary": {
      "type": "string",
      "maxLength": 800
    },
    "lesson_hint": {
      "type": "string",
      "maxLength": 240,
      "description": "The exact concise hint to inject later."
    },
    "trigger_description": {
      "type": "string",
      "maxLength": 500,
      "description": "When this lesson should apply."
    },
    "avoid_when": {
      "type": "array",
      "items": { "type": "string", "maxLength": 240 },
      "maxItems": 5
    },
    "scope": {
      "type": "string",
      "enum": ["project", "repo", "machine", "user", "org", "unknown"]
    },
    "risk_level": {
      "type": "string",
      "enum": ["low", "medium", "high"]
    },
    "confidence": {
      "type": "number",
      "minimum": 0,
      "maximum": 1
    }
  }
}
```

Behavior:

```text
1. Resolve case_id if supplied.
2. Attach recent failed/successful attempts as evidence.
3. Redact again.
4. Store candidate lesson.
5. Return lesson id and status.
```

Output:

```json
{
  "lesson_id": "lesson_abc123",
  "status": "candidate",
  "message": "Recorded candidate Reflex lesson. It will be injected only on close matches until confirmed."
}
```

### `find_lessons`

Purpose:

> Let Codex ask, “Do we know anything about this failure/tool context?”

Input:

```json
{
  "query": "string",
  "tool_name": "string|null",
  "cwd": "string|null",
  "limit": 5
}
```

Output:

```json
{
  "lessons": [
    {
      "lesson_id": "lesson_abc123",
      "status": "active",
      "hint": "Use `uv run pytest` from `services/api` for API tests.",
      "confidence": 0.83,
      "scope": "project"
    }
  ]
}
```

### `mark_lesson_result`

Purpose:

> Codex can confirm or contradict a lesson after using it.

Input:

```json
{
  "lesson_id": "lesson_abc123",
  "result": "confirmed|contradicted|irrelevant",
  "note": "string"
}
```

### `list_recent_cases`

Purpose:

> Debugging and explicit repair registration when the model lacks the case id.

Input:

```json
{
  "limit": 10,
  "status": "open|candidate_repaired|analyzed|any"
}
```

### `ignore_lesson`

Purpose:

> Let the model or user disable a bad lesson.

Input:

```json
{
  "lesson_id": "lesson_abc123",
  "reason": "string"
}
```

---

# 7. Why both hooks and MCP tools?

Use hooks because the model will forget to self-report some repairs.

Use MCP because hooks alone cannot reliably infer causality.

A pure hook system can see:

```text
A failed.
B failed.
C succeeded.
```

But it may not know whether C succeeded because of:

```text
a changed flag
a changed cwd
a dependency install
a login step
a permission approval
external VPN state
a user correction
a completely unrelated success
```

The model-facing MCP tool lets Codex say:

```text
The reusable repair was not the final command itself; it was that I had to run `uv sync` before retrying tests.
```

That is the scalable part.

---

# 8. Episode lifecycle

Use these states:

```text
observed
open
repairing
candidate_repaired
model_registered
analyzed
candidate_lesson
active_lesson
ignored
retired
not_reusable
```

State transitions:

```text
PostToolUse failure
  → open

More related attempts
  → repairing

PostToolUse success near open episode
  → candidate_repaired

MCP register_repair_episode
  → model_registered → candidate_lesson

Stop/analyzer finds reusable repair
  → analyzed → candidate_lesson

Lesson confirmed by later use or user promotion
  → active_lesson

Lesson contradicted repeatedly
  → retired

User disables
  → ignored
```

Default promotion rules:

```text
model_registered once
  → candidate lesson, close-match injection only

candidate lesson injected and later success occurs
  → confidence +0.12

same repair observed independently
  → confidence +0.20

confidence >= 0.78 and at least 2 confirmations
  → active

contradicted once
  → confidence -0.25

contradicted twice
  → demote to candidate

contradicted three times
  → retire
```

---

# 9. Lesson matching

Do not require command regexes. Use a layered matcher.

## 9.1 Cheap retrieval

Index lessons by:

```text
project_hash
repo_remote_hash
scope
tool_name
executable tokens
MCP server/tool name
cwd tokens
lesson text tokens
trigger text tokens
recent error tokens
```

## 9.2 Semantic-lite matching

For v1, do this without embeddings:

```text
BM25/token overlap over:
  pending command
  tool name
  cwd
  user intent excerpt
  trigger_description
  lesson_hint
```

Later, add optional embeddings.

## 9.3 Applicability filter

A lesson can be injected only if:

```text
scope matches
status is candidate or active
risk policy allows hinting
anti-trigger does not match
lesson has not recently failed
confidence exceeds threshold
```

Thresholds:

```text
candidate exact/near match: 0.62
candidate broad match: no inject
active near match: 0.55
active broad match: 0.70
high-risk lesson: +0.15 threshold
```

## 9.4 Injection budget

```text
max lessons: 2
max chars per lesson: 240
max total additionalContext: 600
```

Format:

```text
Reflex: <short scoped lesson>. Avoid if <anti-trigger>.
```

Example:

```text
Reflex: API tests in this repo previously failed from the repo root and later passed from `services/api` with `uv run pytest`. Avoid applying this to frontend tests.
```

---

# 10. Lesson schema

```json
{
  "id": "lesson_01H...",
  "created_at": "2026-06-30T12:00:00Z",
  "updated_at": "2026-06-30T12:00:00Z",
  "status": "candidate",
  "scope": {
    "level": "project",
    "project_hash": "sha256:...",
    "repo_remote_hash": "sha256:...",
    "machine_hash": "sha256:..."
  },
  "trigger": {
    "description": "When running API tests in this repository.",
    "tool_names": ["Bash"],
    "positive_terms": ["pytest", "api", "tests", "uv"],
    "negative_terms": ["frontend", "e2e"],
    "cwd_hints": ["services/api"],
    "semantic_intent": "run Python API tests"
  },
  "lesson": {
    "hint": "API tests previously passed from `services/api` using `uv run pytest`.",
    "repair_type": "changed_cwd_and_tool_prefix",
    "risk_level": "low",
    "rewrite_allowed": false
  },
  "evidence": {
    "case_ids": ["case_01H..."],
    "failed_attempt_ids": ["attempt_001"],
    "successful_attempt_ids": ["attempt_004"],
    "model_registered": true
  },
  "confidence": 0.68,
  "times_injected": 0,
  "times_confirmed": 1,
  "times_contradicted": 0,
  "last_injected_at": null,
  "last_confirmed_at": "2026-06-30T12:00:00Z"
}
```

---

# 11. Event schema

## `tool_attempt`

```json
{
  "id": "attempt_01H...",
  "session_id": "string",
  "turn_id": "string|null",
  "tool_use_id": "string|null",
  "ts": "2026-06-30T12:00:00Z",
  "cwd": "/repo",
  "project_hash": "sha256:...",
  "tool_name": "Bash",
  "tool_input_redacted": {
    "command": "aws eks describe-cluster --name prod"
  },
  "tool_response_summary": {
    "exit_code": 255,
    "result": "failure",
    "stdout_excerpt": "",
    "stderr_excerpt": "Unable to locate credentials",
    "duration_ms": 1180
  },
  "classification": {
    "result": "failure|success|unknown",
    "failure_kind": "auth|permission|network|tool_error|test_failure|unknown",
    "source": "heuristic"
  },
  "raw_ref": "events/2026-06-30/session_id/attempt_01H.json"
}
```

## `repair_episode`

```json
{
  "id": "case_01H...",
  "status": "open",
  "session_id": "string",
  "turn_id": "string|null",
  "project_hash": "sha256:...",
  "user_intent_excerpt": "string|null",
  "opened_by_attempt_id": "attempt_001",
  "attempt_ids": ["attempt_001", "attempt_002"],
  "permission_event_ids": [],
  "created_at": "2026-06-30T12:00:00Z",
  "expires_at": "2026-06-30T14:00:00Z",
  "resolution": null
}
```

---

# 12. Redaction rules

Redact before storage and before any model/analyzer call.

Must redact:

```text
API keys
bearer tokens
cookies
private keys
SSH keys
AWS secret keys
GitHub tokens
OAuth codes
one-time URLs
passwords
session IDs where secret-like
Authorization headers
.env values except allowlisted names
```

Optional hash:

```text
AWS account ids
internal hostnames
email addresses
usernames
cluster names
database names
```

Do not store full raw output by default. Store:

```text
exit code
first N stderr chars
first N stdout chars
output hashes
redacted excerpts
```

Config:

```toml
[reflex.privacy]
store_raw = false
max_stdout_chars = 2000
max_stderr_chars = 4000
hash_account_ids = true
hash_hostnames = true
```

---

# 13. Automatic analyzer

The automatic analyzer should be optional but designed into the architecture.

## 13.1 When it runs

```text
on Stop
on explicit `reflex analyze`
optionally after candidate_repaired if config allows
```

Avoid long model calls inside normal hooks. Hooks are synchronous and async command hooks are currently skipped, so the hot path should only enqueue analysis. ([OpenAI Developers][2])

## 13.2 Analyzer input

Give the analyzer a compact, redacted episode:

```json
{
  "case_id": "case_01H...",
  "task_intent": "Run API tests",
  "events": [
    {
      "id": "attempt_001",
      "kind": "tool_attempt",
      "tool_name": "Bash",
      "input": "pytest tests/test_auth.py",
      "result": "failure",
      "output_excerpt": "ModuleNotFoundError: No module named ..."
    },
    {
      "id": "attempt_002",
      "kind": "tool_attempt",
      "tool_name": "Bash",
      "input": "cat pyproject.toml",
      "result": "success",
      "output_excerpt": "[tool.uv] ..."
    },
    {
      "id": "attempt_003",
      "kind": "tool_attempt",
      "tool_name": "Bash",
      "input": "uv sync",
      "result": "success",
      "output_excerpt": "Installed ..."
    },
    {
      "id": "attempt_004",
      "kind": "tool_attempt",
      "tool_name": "Bash",
      "input": "uv run pytest tests/test_auth.py",
      "result": "success",
      "output_excerpt": "1 passed"
    }
  ]
}
```

## 13.3 Analyzer output

Use structured JSON. OpenAI’s Structured Outputs feature is designed to make model responses adhere to a supplied JSON Schema, which is appropriate here because the plugin needs machine-consumable lesson candidates rather than freeform summaries. ([OpenAI Developers][4])

```json
{
  "case_status": "repaired",
  "reusable": true,
  "goal": "Run API tests",
  "failure_summary": "Tests failed due to missing environment/dependencies.",
  "repair_summary": "The agent ran uv sync and then uv run pytest.",
  "minimal_causal_repair": {
    "type": "ran_prerequisite_and_changed_invocation",
    "description": "Use uv-managed environment for Python tests.",
    "confidence": 0.82
  },
  "lesson": {
    "scope": "project",
    "hint": "Python tests in this repo previously required the uv environment; use `uv run pytest`, and run `uv sync` if imports are missing.",
    "trigger_description": "Running Python tests in this repository.",
    "avoid_when": [
      "Do not apply to Node/frontend tests.",
      "Do not run uv sync for assertion failures unrelated to imports."
    ],
    "risk_level": "low",
    "rewrite_allowed": false
  },
  "evidence": {
    "failed_attempt_ids": ["attempt_001"],
    "diagnostic_attempt_ids": ["attempt_002"],
    "successful_attempt_ids": ["attempt_003", "attempt_004"]
  },
  "should_store": true
}
```

---

# 14. Plugin structure

```text
reflex-for-codex/
  .codex-plugin/
    plugin.json

  .mcp.json

  hooks/
    hooks.json

  skills/
    reflex/
      SKILL.md
      agents/
        openai.yaml

  src/
    bin/
      reflex.rs
      reflex_mcp.rs

    hooks/
      session_start.rs
      user_prompt_submit.rs
      pre_tool_use.rs
      post_tool_use.rs
      permission_request.rs
      stop.rs

    mcp/
      server.rs
      tools_register_repair_episode.rs
      tools_find_lessons.rs
      tools_mark_lesson_result.rs
      tools_list_recent_cases.rs
      tools_ignore_lesson.rs

    storage/
      db.rs
      migrations.rs
      project.rs
      paths.rs

    episode/
      builder.rs
      resolver.rs
      analyzer.rs
      schemas.rs

    retrieval/
      index.rs
      match_lessons.rs
      inject.rs

    privacy/
      redact.rs
      secrets.rs

    cli/
      lessons.rs
      status.rs
      analyze.rs
      doctor.rs

  migrations/
    001_init.sql

  examples/
    wrong-cwd-demo/
    package-manager-demo/
    explicit-repair-registration-demo/

  docs/
    ARCHITECTURE.md
    PRIVACY.md
    DEMOS.md

  README.md
  Cargo.toml
```

---

# 15. Plugin manifest

```json
{
  "name": "reflex",
  "version": "0.1.0",
  "description": "Operational repair memory for OpenAI Codex CLI tool calls.",
  "author": {
    "name": "James Kassemi"
  },
  "license": "MIT OR Apache-2.0",
  "keywords": ["codex", "plugin", "mcp", "tool-use", "memory"],
  "skills": "./skills/",
  "mcpServers": "./.mcp.json",
  "interface": {
    "displayName": "Reflex",
    "shortDescription": "Codex learns from failed tool calls.",
    "longDescription": "Reflex records failed tool-call episodes, lets Codex register reusable repairs, and injects concise scoped hints before similar future tool calls.",
    "developerName": "James Kassemi",
    "category": "Developer Tools",
    "capabilities": ["Read", "Write"]
  }
}
```

Reflex uses the default plugin-bundled hook location `hooks/hooks.json`; the plugin manifest intentionally omits a `hooks` field for validator compatibility.

Plugin structure and manifest paths should follow Codex’s plugin packaging rules: `.codex-plugin/plugin.json` is the required entry point, while `skills`, `mcpServers`, and `apps` point to plugin-root-relative components. ([OpenAI Developers][1])

## 15.1 Hook discovery compatibility

`hooks/hooks.json` remains required in the package, but `.codex-plugin/plugin.json` should not point to it by default. Codex discovers `hooks/hooks.json` automatically when the plugin is enabled.

The manifest `hooks` field is only needed to override the default hook location or use multiple/inline hook definitions. Because local plugin-creator validation may reject the explicit field, v1 omits it unless local install validation proves it is accepted.

---

# 16. Hooks config

```json
{
  "hooks": {
    "SessionStart": [
      {
        "matcher": "startup|resume|clear",
        "hooks": [
          {
            "type": "command",
            "command": "${PLUGIN_ROOT}/bin/reflex hook session-start",
            "timeout": 2,
            "statusMessage": "Starting Reflex"
          }
        ]
      }
    ],
    "UserPromptSubmit": [
      {
        "hooks": [
          {
            "type": "command",
            "command": "${PLUGIN_ROOT}/bin/reflex hook user-prompt-submit",
            "timeout": 2,
            "statusMessage": "Recording task intent"
          }
        ]
      }
    ],
    "PreToolUse": [
      {
        "matcher": "Bash|apply_patch|mcp__.*",
        "hooks": [
          {
            "type": "command",
            "command": "${PLUGIN_ROOT}/bin/reflex hook pre-tool-use",
            "timeout": 2,
            "statusMessage": "Checking Reflex"
          }
        ]
      }
    ],
    "PostToolUse": [
      {
        "matcher": "Bash|apply_patch|mcp__.*",
        "hooks": [
          {
            "type": "command",
            "command": "${PLUGIN_ROOT}/bin/reflex hook post-tool-use",
            "timeout": 3,
            "statusMessage": "Recording Reflex result"
          }
        ]
      }
    ],
    "PermissionRequest": [
      {
        "matcher": "Bash|apply_patch|mcp__.*",
        "hooks": [
          {
            "type": "command",
            "command": "${PLUGIN_ROOT}/bin/reflex hook permission-request",
            "timeout": 2,
            "statusMessage": "Checking Reflex approval memory"
          }
        ]
      }
    ],
    "Stop": [
      {
        "hooks": [
          {
            "type": "command",
            "command": "${PLUGIN_ROOT}/bin/reflex hook stop",
            "timeout": 5,
            "statusMessage": "Finalizing Reflex episodes"
          }
        ]
      }
    ]
  }
}
```

Plugin-bundled hooks are not automatically trusted; installing or enabling Reflex does not by itself make bundled hooks run. Codex skips non-managed plugin hooks until the user reviews and trusts the current hook definition, so the README installation steps must require users to review and trust the Reflex hook definitions before expecting hooks to run. If hooks do not appear to run, check hook trust and enablement first. ([OpenAI Developers][1])

---

# 17. MCP config

```json
{
  "mcp_servers": {
    "reflex": {
      "command": "reflex-mcp",
      "args": ["--stdio"]
    }
  }
}
```

If relative binary resolution is unreliable in practice, the installer should generate or patch this to an absolute path inside the installed plugin cache, or use a wrapper script under `${PLUGIN_ROOT}` if Codex’s plugin MCP launcher supports expansion for the command field in your target version. The docs clearly show plugin-bundled `.mcp.json` support, but they do not spell out `${PLUGIN_ROOT}` expansion for MCP command fields the way they do for hook commands, so implementation should verify this locally. ([OpenAI Developers][1])

---

# 18. Skill instructions

`skills/reflex/SKILL.md`:

```markdown
# Reflex

Use Reflex to preserve reusable operational repairs from failed tool calls.

When a tool call fails, do not immediately record a lesson. Continue diagnosing normally.

After you discover a correction and a later attempt succeeds, call the Reflex MCP tool `register_repair_episode` if the correction is likely reusable. Good examples include:
- corrected CLI flags
- correct profile/region/context
- required working directory
- required package manager
- prerequisite install/setup command
- permission/escalation pattern
- MCP argument-shape correction

Do not record:
- secrets, tokens, private keys, cookies, one-time URLs
- broad rules like “always use sudo”
- ordinary code/test failures whose fix was changing product code
- guesses that have not succeeded

Keep lessons scoped, concise, and falsifiable.
```

Codex skills are reusable instructions, and plugins can bundle one or more skills; Codex’s docs recommend focused skills with explicit inputs and outputs. ([OpenAI Developers][5])

---

# 19. CLI

Ship a CLI mainly for inspection and trust.

```bash
reflex status
reflex lessons
reflex lesson <id>
reflex cases
reflex case <id>
reflex analyze
reflex ignore <lesson-id>
reflex promote <lesson-id>
reflex demote <lesson-id>
reflex doctor
reflex export --format json
reflex purge --older-than 30d
```

Example:

```text
$ reflex lessons

Active lessons for this repo:

lesson_01HABC  confidence 0.84  confirmed 3x
  Trigger: Python API tests
  Hint: API tests previously passed from `services/api` using `uv run pytest`.

lesson_01HDEF  confidence 0.71  candidate
  Trigger: AWS EKS inspection
  Hint: Similar AWS EKS commands succeeded with `--profile platform-admin --region us-east-1`.
```

---

# 20. Storage

Use SQLite under `PLUGIN_DATA`.

```text
$PLUGIN_DATA/
  reflex.db
  events/
    <session_id>/*.jsonl
  projects/
    <project_hash>/
      REFLEX.md
      exports/
```

Tables:

```sql
create table tool_attempts (
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

create table repair_episodes (
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

create table lessons (
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

create table lesson_injections (
  id text primary key,
  lesson_id text not null,
  attempt_id text,
  session_id text not null,
  ts text not null,
  pending_tool_summary_json text not null,
  subsequent_result text
);
```

---

# 21. Safety policy

Default mode:

```toml
[reflex]
mode = "hint" # observe | hint | block | rewrite

[reflex.permissions]
auto_allow = false

[reflex.analyzer]
enabled = false
provider = "none" # none | openai
run_on_stop = false
```

Modes:

```text
observe:
  record only

hint:
  inject concise additionalContext only

block:
  optionally block known-bad repeated commands with explanation

rewrite:
  only for explicitly allowlisted low-risk transformations
```

High-risk lessons should never auto-rewrite by default:

```text
aws profile changes
kube context changes
terraform workspace changes
sudo/escalation
deploy/apply/delete/migrate commands
production/staging target changes
```

---

# 22. Acceptance tests

Validation and install acceptance:

```text
hooks/hooks.json exists and validates.
plugin.json validates without a hooks field.
Local Codex install/validation succeeds.
README hook trust/install documentation is present.
After hook trust, SessionStart, PreToolUse, PostToolUse, PermissionRequest, and Stop hooks are callable.
```

Build fixtures for three demos.

## Demo A: wrong working directory

Sequence:

```text
pytest tests/test_auth.py
  → fails: file/import/path issue

cd services/api && uv run pytest tests/test_auth.py
  → succeeds
```

Expected:

```text
MCP registration creates candidate lesson.
Future `pytest tests/test_auth.py` from repo root gets Reflex hint.
```

## Demo B: package manager substitution

Sequence:

```text
npm test
  → fails

pnpm test
  → succeeds
```

Expected:

```text
Lesson: this repo uses pnpm for scripts.
```

## Demo C: explicit model registration

Mock Codex calls:

```text
register_repair_episode(case_id, failure_summary, repair_summary, lesson_hint, ...)
```

Expected:

```text
Lesson stored.
Lesson listed by CLI.
Lesson injected by PreToolUse on close match.
```

## Demo D: secrets redaction

Input:

```text
Authorization: Bearer abc123
AWS_SECRET_ACCESS_KEY=...
```

Expected:

```text
No stored event, lesson, analyzer payload, or audit file contains raw secret.
```

## Demo E: no over-learning from code fixes

Sequence:

```text
pytest
  → assertion fails

apply_patch product code
pytest
  → succeeds
```

Expected:

```text
No operational lesson unless the model explicitly registers a valid operational repair.
```

# 23. Important implementation detail

When Codex hits a failure, **do not make Reflex ask the model “what happened?” immediately.**

Instead:

```text
Failure:
  record and open episode

More failures/diagnostics:
  attach to episode

Success:
  model may explicitly register repair via MCP
  or plugin marks episode candidate_repaired

Stop / analyze:
  optional analyzer proposes lesson

Future similar command:
  PreToolUse injects concise hint
```
