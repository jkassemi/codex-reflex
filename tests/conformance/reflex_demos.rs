use reflex::domain;
use reflex::hooks;
use reflex::mcp::{self, RegisterRepairEpisodeInput};
use reflex::storage::Store;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::fs;

fn temp_project() -> (tempfile::TempDir, std::path::PathBuf, Store) {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path().join("repo");
    fs::create_dir_all(root.join(".git")).expect("git dir");
    fs::create_dir_all(root.join("services/api")).expect("api dir");
    let store = Store::open(temp.path().join("data")).expect("store");
    (temp, root, store)
}

fn bash_event(session: &str, cwd: &std::path::Path, command: &str, exit_code: i64, stderr: &str) -> Value {
    json!({
        "session_id": session,
        "tool_name": "Bash",
        "cwd": cwd.to_string_lossy(),
        "tool_input": {"command": command},
        "tool_response": {"exit_code": exit_code, "stdout": "", "stderr": stderr}
    })
}

fn codex_bash_output(exit_code: i64, output: &str) -> Value {
    json!({
        "output": format!(
            "Chunk ID: abc123\nWall time: 0.1000 seconds\nProcess exited with code {exit_code}\nOriginal token count: 1\nOutput:\n{output}"
        )
    })
}

fn bash_predicates(command_family: &str, executables: &[&str]) -> domain::LessonPredicates {
    domain::LessonPredicates {
        tool_family: Some("Bash".to_string()),
        command_family: Some(command_family.to_string()),
        match_executables: executables.iter().map(|value| value.to_string()).collect(),
        ..Default::default()
    }
}

fn pytest_env_predicates(root: &std::path::Path) -> domain::LessonPredicates {
    let mut required_env = BTreeMap::new();
    required_env.insert("PYTHONPATH".to_string(), root.to_string_lossy().to_string());
    domain::LessonPredicates {
        tool_family: Some("Bash".to_string()),
        command_family: Some("pytest".to_string()),
        match_executables: vec!["pytest".to_string(), "./.venv/bin/pytest".to_string()],
        required_env: required_env.clone(),
        required_cwd: Some(root.to_string_lossy().to_string()),
        preferred_command: Some(format!(
            "PYTHONPATH={} ./.venv/bin/pytest",
            root.display()
        )),
        suppress_when: domain::PredicateConditions {
            env: required_env,
            ..Default::default()
        },
        ..Default::default()
    }
}

// SPEC: Demo A, sections 3, 8, 9, 22
#[test]
fn wrong_cwd_demo_registers_and_injects_close_match() {
    let (_temp, root, store) = temp_project();
    let failed = bash_event(
        "s1",
        &root,
        "pytest tests/test_auth.py",
        2,
        "file not found: tests/test_auth.py",
    );
    let first_output = hooks::post_tool_use(&store, &failed).expect("record failure");
    let case_id = first_output["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .expect("case context")
        .split("case ")
        .nth(1)
        .expect("case id suffix")
        .split('.')
        .next()
        .expect("case id")
        .to_string();

    let passed = bash_event(
        "s1",
        &root.join("services/api"),
        "uv run pytest tests/test_auth.py",
        0,
        "",
    );
    hooks::post_tool_use(&store, &passed).expect("record success");

    mcp::register_repair_episode(
        &store,
        RegisterRepairEpisodeInput {
            case_id: Some(case_id),
            reusable: true,
            failure_summary: "pytest failed from the repository root with a path error".to_string(),
            repair_summary: "tests passed from services/api with uv run pytest".to_string(),
            lesson_hint: "API tests in this repo previously passed from `services/api` using `uv run pytest`.".to_string(),
            trigger_description: "Running pytest API tests from this repository.".to_string(),
            avoid_when: vec!["frontend tests".to_string()],
            predicates: bash_predicates("pytest", &["pytest", "uv"]),
            scope: "project".to_string(),
            risk_level: "low".to_string(),
            confidence: 0.72,
        },
    )
    .expect("register lesson");

    let pre = json!({
        "session_id": "s2",
        "tool_name": "Bash",
        "cwd": root.to_string_lossy(),
        "tool_input": {"command": "pytest tests/test_auth.py"}
    });
    let hint = hooks::pre_tool_use(&store, &pre).expect("pre hook");
    let context = hint["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .expect("hint context");
    assert!(hint.get("decision").is_none(), "{hint}");
    assert!(context.contains("services/api"), "{context}");
    assert!(context.contains("uv run pytest"), "{context}");
}

// SPEC: Demo B, sections 9, 22
#[test]
fn package_manager_demo_injects_pnpm_hint_for_npm_test() {
    let (_temp, root, store) = temp_project();
    let failed = bash_event("pm", &root, "npm test", 1, "missing pnpm workspace script");
    let first_output = hooks::post_tool_use(&store, &failed).expect("record npm failure");
    let case_id = first_output["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .expect("case context")
        .split("case ")
        .nth(1)
        .expect("case id suffix")
        .split('.')
        .next()
        .expect("case id")
        .to_string();
    let passed = bash_event("pm", &root, "pnpm test", 0, "");
    hooks::post_tool_use(&store, &passed).expect("record pnpm success");
    mcp::register_repair_episode(
        &store,
        RegisterRepairEpisodeInput {
            case_id: Some(case_id),
            reusable: true,
            failure_summary: "npm test failed because this repository does not use npm scripts directly.".to_string(),
            repair_summary: "pnpm test succeeded.".to_string(),
            lesson_hint: "This repo previously failed with `npm test` and passed with `pnpm test`.".to_string(),
            trigger_description: "Running npm or JavaScript package test scripts.".to_string(),
            avoid_when: vec![],
            predicates: bash_predicates("npm", &["npm"]),
            scope: "project".to_string(),
            risk_level: "low".to_string(),
            confidence: 0.8,
        },
    )
    .expect("register package manager lesson");

    let pre = json!({
        "session_id": "s3",
        "tool_name": "Bash",
        "cwd": root.to_string_lossy(),
        "tool_input": {"command": "npm test"}
    });
    let hint = hooks::pre_tool_use(&store, &pre).expect("pre hook");
    let context = hint["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .expect("hint context");
    assert!(hint.get("decision").is_none(), "{hint}");
    assert!(context.contains("pnpm test"), "{context}");
}

// SPEC: Demo C, sections 6, 19, 22
#[test]
fn explicit_registration_stores_lesson_and_cli_lists_it() {
    let temp = tempfile::tempdir().expect("tempdir");
    let data = temp.path().join("data");
    std::env::set_var("REFLEX_DATA", &data);
    let store = Store::open(&data).expect("store");
    let registered = mcp::register_repair_episode(
        &store,
        RegisterRepairEpisodeInput {
            case_id: None,
            reusable: true,
            failure_summary: "aws eks failed without explicit region".to_string(),
            repair_summary: "aws eks worked with --region us-east-1".to_string(),
            lesson_hint: "Similar AWS EKS inspection previously required `--region us-east-1`.".to_string(),
            trigger_description: "Inspecting AWS EKS clusters.".to_string(),
            avoid_when: vec!["non-AWS commands".to_string()],
            predicates: bash_predicates("aws", &["aws"]),
            scope: "project".to_string(),
            risk_level: "medium".to_string(),
            confidence: 0.9,
        },
    )
    .expect("register");
    let lesson_id = registered["lesson_id"].as_str().expect("lesson id");

    let mut output = Vec::new();
    reflex::cli::run(vec!["lessons".to_string()], &mut std::io::empty(), &mut output).expect("cli");
    let text = String::from_utf8(output).expect("utf8");
    assert!(text.contains(lesson_id), "{text}");
    assert!(text.contains("AWS EKS"), "{text}");
}

// SPEC: sections 6, 17
#[test]
fn explicit_registration_without_case_uses_repo_path_from_trigger() {
    let (_temp, root, store) = temp_project();
    mcp::register_repair_episode(
        &store,
        RegisterRepairEpisodeInput {
            case_id: None,
            reusable: true,
            failure_summary: "pytest collection failed with ModuleNotFoundError".to_string(),
            repair_summary: "pytest collected after PYTHONPATH included the repo root".to_string(),
            lesson_hint: format!(
                "Run pytest with the repo root on PYTHONPATH, e.g. `PYTHONPATH={} .venv/bin/pytest`.",
                root.display()
            ),
            trigger_description: format!("Running pytest in {} via .venv/bin/pytest", root.display()),
            avoid_when: vec![],
            predicates: pytest_env_predicates(&root),
            scope: "repo".to_string(),
            risk_level: "low".to_string(),
            confidence: 0.9,
        },
    )
    .expect("register lesson");

    let lessons = store.list_lessons(10).expect("lessons");
    assert_eq!(lessons.len(), 1);
    assert_eq!(lessons[0].project_hash, domain::project_hash(&root.to_string_lossy()));

    let pre = json!({
        "session_id": "explicit-no-case",
        "tool_name": "Bash",
        "cwd": root.to_string_lossy(),
        "tool_input": {"command": ".venv/bin/pytest"}
    });
    let blocked = hooks::pre_tool_use(&store, &pre).expect("pre hook");
    assert_eq!(blocked["decision"], "block", "{blocked}");
    assert!(blocked.get("hookSpecificOutput").is_none(), "{blocked}");
    assert!(
        blocked["reason"]
            .as_str()
            .expect("reason")
            .contains("Run `PYTHONPATH="),
        "{blocked}"
    );
    assert!(
        blocked["reason"]
            .as_str()
            .expect("reason")
            .contains("Do not call register_repair_episode"),
        "{blocked}"
    );
}

// SPEC: section 9
#[test]
fn pytest_lesson_matches_command_profile_not_cwd_noise() {
    let (_temp, root, store) = temp_project();
    mcp::register_repair_episode(
        &store,
        RegisterRepairEpisodeInput {
            case_id: None,
            reusable: true,
            failure_summary: "pytest collection failed with ModuleNotFoundError".to_string(),
            repair_summary: "pytest collected after PYTHONPATH included the repo root".to_string(),
            lesson_hint: format!(
                "For this repo, run pytest as PYTHONPATH={} ./.venv/bin/pytest from the repo root.",
                root.display()
            ),
            trigger_description: format!(
                "Running pytest from {} with ./.venv/bin/pytest failed during collection.",
                root.display()
            ),
            avoid_when: vec!["The package has been installed editable.".to_string()],
            predicates: pytest_env_predicates(&root),
            scope: "repo".to_string(),
            risk_level: "low".to_string(),
            confidence: 0.95,
        },
    )
    .expect("register lesson");

    let broken = json!({
        "session_id": "pytest-profile",
        "tool_name": "Bash",
        "cwd": root.to_string_lossy(),
        "tool_input": {"command": "./.venv/bin/pytest tests/test_auth.py -q --tb=short"}
    });
    let blocked = hooks::pre_tool_use(&store, &broken).expect("pre hook");
    assert_eq!(blocked["decision"], "block", "{blocked}");
    assert!(blocked.get("hookSpecificOutput").is_none(), "{blocked}");
    assert!(
        blocked["reason"]
            .as_str()
            .expect("reason")
            .contains("PYTHONPATH"),
        "{blocked}"
    );

    let already_fixed = json!({
        "session_id": "pytest-profile",
        "tool_name": "Bash",
        "cwd": root.to_string_lossy(),
        "tool_input": {"command": format!("PYTHONPATH={} ./.venv/bin/pytest", root.display())}
    });
    let output = hooks::pre_tool_use(&store, &already_fixed).expect("pre hook");
    assert!(output.as_object().expect("object").is_empty(), "{output}");

    let unrelated_same_cwd = json!({
        "session_id": "pytest-profile",
        "tool_name": "Bash",
        "cwd": root.to_string_lossy(),
        "tool_input": {"command": "sed -n '1,220p' /tmp/SKILL.md"}
    });
    let output = hooks::pre_tool_use(&store, &unrelated_same_cwd).expect("pre hook");
    assert!(output.as_object().expect("object").is_empty(), "{output}");

    let mcp_call = json!({
        "session_id": "pytest-profile",
        "tool_name": "mcp__reflex__register_repair_episode",
        "cwd": root.to_string_lossy(),
        "tool_input": {"trigger_description": "Running pytest from repo root"}
    });
    let output = hooks::pre_tool_use(&store, &mcp_call).expect("pre hook");
    assert!(output.as_object().expect("object").is_empty(), "{output}");
}

// SPEC: section 9
#[test]
fn structured_predicates_drive_pytest_matching() {
    let (_temp, root, store) = temp_project();
    let mut predicates = pytest_env_predicates(&root);
    predicates.match_executables = vec!["./.venv/bin/pytest".to_string()];
    predicates.suppress_when.command_contains = vec![".venv/bin/pytest".to_string()];
    mcp::register_repair_episode(
        &store,
        RegisterRepairEpisodeInput {
            case_id: None,
            reusable: true,
            failure_summary: "plain pytest is not on PATH".to_string(),
            repair_summary: "pytest ran through the venv with PYTHONPATH".to_string(),
            lesson_hint: format!(
                "Run pytest with `PYTHONPATH={} ./.venv/bin/pytest`.",
                root.display()
            ),
            trigger_description: format!("Running pytest from {}", root.display()),
            avoid_when: vec![],
            predicates,
            scope: "repo".to_string(),
            risk_level: "low".to_string(),
            confidence: 0.95,
        },
    )
    .expect("register lesson");

    let stored = store.list_lessons(10).expect("lessons");
    assert!(stored[0].trigger_json.get("predicates").is_some());

    let bare = json!({
        "session_id": "structured-pytest",
        "tool_name": "Bash",
        "cwd": root.to_string_lossy(),
        "tool_input": {"command": "pytest tests/test_auth.py -q"}
    });
    let blocked = hooks::pre_tool_use(&store, &bare).expect("pre hook");
    assert_eq!(blocked["decision"], "block", "{blocked}");
    assert!(blocked.get("hookSpecificOutput").is_none(), "{blocked}");
    assert!(
        blocked["reason"].as_str().expect("reason").contains(&format!(
            "Run `PYTHONPATH={} ./.venv/bin/pytest`",
            root.display()
        )),
        "{blocked}"
    );

    let venv_without_env = json!({
        "session_id": "structured-pytest",
        "tool_name": "Bash",
        "cwd": root.to_string_lossy(),
        "tool_input": {"command": "./.venv/bin/pytest"}
    });
    let blocked = hooks::pre_tool_use(&store, &venv_without_env).expect("pre hook");
    assert_eq!(blocked["decision"], "block", "{blocked}");
    assert!(
        blocked["reason"]
            .as_str()
            .expect("reason")
            .contains("PYTHONPATH"),
        "{blocked}"
    );

    let satisfied = json!({
        "session_id": "structured-pytest",
        "tool_name": "Bash",
        "cwd": root.to_string_lossy(),
        "tool_input": {"command": format!("PYTHONPATH={} ./.venv/bin/pytest", root.display())}
    });
    assert!(
        hooks::pre_tool_use(&store, &satisfied)
            .expect("pre hook")
            .as_object()
            .expect("object")
            .is_empty()
    );

    let unrelated = json!({
        "session_id": "structured-pytest",
        "tool_name": "Bash",
        "cwd": root.to_string_lossy(),
        "tool_input": {"command": "sed -n '1,220p' README.md"}
    });
    assert!(
        hooks::pre_tool_use(&store, &unrelated)
            .expect("pre hook")
            .as_object()
            .expect("object")
            .is_empty()
    );
}

// SPEC: section 9
#[test]
fn registration_requires_specific_predicates() {
    let (_temp, _root, store) = temp_project();
    let result = mcp::register_repair_episode(
        &store,
        RegisterRepairEpisodeInput {
            case_id: None,
            reusable: true,
            failure_summary: "command failed".to_string(),
            repair_summary: "command passed".to_string(),
            lesson_hint: "Use the corrected command.".to_string(),
            trigger_description: "Running shell commands.".to_string(),
            avoid_when: vec![],
            predicates: domain::LessonPredicates {
                tool_family: Some("Bash".to_string()),
                ..Default::default()
            },
            scope: "repo".to_string(),
            risk_level: "low".to_string(),
            confidence: 0.9,
        },
    );
    assert!(result.is_err());
    assert!(store.list_lessons(10).expect("lessons").is_empty());
}

// SPEC: section 9
#[test]
fn duplicate_operational_registration_reuses_existing_lesson() {
    let (_temp, root, store) = temp_project();
    let first = mcp::register_repair_episode(
        &store,
        RegisterRepairEpisodeInput {
            case_id: None,
            reusable: true,
            failure_summary: "pytest failed because local modules were not importable".to_string(),
            repair_summary: "pytest worked after setting PYTHONPATH to the repo root".to_string(),
            lesson_hint: format!(
                "Run pytest as PYTHONPATH={} ./.venv/bin/pytest.",
                root.display()
            ),
            trigger_description: format!("Running pytest from {}", root.display()),
            avoid_when: vec![],
            predicates: pytest_env_predicates(&root),
            scope: "repo".to_string(),
            risk_level: "low".to_string(),
            confidence: 0.95,
        },
    )
    .expect("first lesson");
    let second = mcp::register_repair_episode(
        &store,
        RegisterRepairEpisodeInput {
            case_id: None,
            reusable: true,
            failure_summary: "pytest collection failed without repo imports".to_string(),
            repair_summary: "collection worked with PYTHONPATH on the venv pytest command".to_string(),
            lesson_hint: format!(
                "For this repo, run pytest with PYTHONPATH={} ./.venv/bin/pytest.",
                root.display()
            ),
            trigger_description: format!(
                "Running ./.venv/bin/pytest in {} without PYTHONPATH.",
                root.display()
            ),
            avoid_when: vec![],
            predicates: pytest_env_predicates(&root),
            scope: "repo".to_string(),
            risk_level: "low".to_string(),
            confidence: 0.95,
        },
    )
    .expect("duplicate lesson");

    assert_eq!(first["lesson_id"], second["lesson_id"]);
    assert_eq!(store.list_lessons(10).expect("lessons").len(), 1);
}

// SPEC: sections 6, 17
#[test]
fn mcp_tools_are_visible() {
    let tools = mcp::tool_metadata();
    let text = serde_json::to_string(&tools).expect("json");
    assert!(text.contains("register_repair_episode"));
    assert!(text.contains("find_lessons"));
    assert!(text.contains("mark_lesson_result"));
    assert!(text.contains("list_recent_cases"));
    assert!(text.contains("ignore_lesson"));
}

// SPEC: sections 3.2, 4.2
#[test]
fn codex_bash_output_shape_opens_and_repairs_cases() {
    let (_temp, root, store) = temp_project();
    let failed = json!({
        "session_id": "real-shape",
        "tool_name": "Bash",
        "cwd": root.to_string_lossy(),
        "tool_input": {"command": "cargo test"},
        "tool_response": codex_bash_output(101, "error: test failed")
    });
    let output = hooks::post_tool_use(&store, &failed).expect("record failure");
    let context = output["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .expect("case context");
    assert!(context.contains("failure recorded as case"), "{context}");

    let attempts = store.list_attempts(10).expect("attempts");
    assert_eq!(attempts[0].result.as_str(), "failure");
    assert_eq!(attempts[0].tool_response_summary_json["exit_code"], 101);
    assert!(
        attempts[0].tool_response_summary_json["stderr_excerpt"]
            .as_str()
            .expect("stderr excerpt")
            .contains("test failed")
    );

    let passed = json!({
        "session_id": "real-shape",
        "tool_name": "Bash",
        "cwd": root.to_string_lossy(),
        "tool_input": {"command": "cargo test"},
        "tool_response": codex_bash_output(0, "test result: ok")
    });
    hooks::post_tool_use(&store, &passed).expect("record success");
    let cases = store.list_cases(Some("candidate_repaired"), 10).expect("cases");
    assert_eq!(cases.len(), 1);
}

// SPEC: sections 3.2, 4.2
#[test]
fn nested_metadata_exit_code_shape_is_classified() {
    let (_temp, root, store) = temp_project();
    let event = json!({
        "session_id": "metadata-shape",
        "tool_name": "Bash",
        "cwd": root.to_string_lossy(),
        "tool_input": {"command": "false"},
        "tool_response": {
            "output": "command failed",
            "metadata": {"exit_code": 1, "duration_seconds": 0.0}
        }
    });
    hooks::post_tool_use(&store, &event).expect("record failure");
    let attempts = store.list_attempts(10).expect("attempts");
    assert_eq!(attempts[0].result.as_str(), "failure");
    assert_eq!(attempts[0].tool_response_summary_json["exit_code"], 1);
}

// SPEC: section 4.3
#[test]
fn permission_request_defers_by_omitting_decision() {
    let (_temp, root, store) = temp_project();
    let event = json!({
        "session_id": "permission-shape",
        "tool_name": "Bash",
        "cwd": root.to_string_lossy(),
        "tool_input": {"command": "git push"}
    });
    let output = hooks::permission_request(&store, &event).expect("permission request");
    assert!(output.get("decision").is_none(), "{output}");
    assert!(output.as_object().expect("object").is_empty(), "{output}");
}

// SPEC: Demo D, section 12, 22
#[test]
fn secrets_redaction_demo_does_not_store_raw_secrets() {
    let (_temp, root, store) = temp_project();
    let event = json!({
        "session_id": "secret-session",
        "tool_name": "Bash",
        "cwd": root.to_string_lossy(),
        "tool_input": {
            "command": "curl -H 'Authorization: Bearer abc123' https://example.test",
            "AWS_SECRET_ACCESS_KEY": "supersecret"
        },
        "tool_response": {
            "exit_code": 1,
            "stderr": "Authorization: Bearer abc123\nAWS_SECRET_ACCESS_KEY=supersecret"
        }
    });
    hooks::post_tool_use(&store, &event).expect("record");
    let stored = serde_json::to_string(&store.list_attempts(10).expect("attempts")).expect("json");
    assert!(!stored.contains("Bearer abc123"), "{stored}");
    assert!(!stored.contains("supersecret"), "{stored}");
    assert!(stored.contains("[REDACTED]"), "{stored}");
}

// SPEC: Demo E, sections 3.2, 22
#[test]
fn product_code_fix_does_not_create_operational_lesson_without_registration() {
    let (_temp, root, store) = temp_project();
    let failed = bash_event("codefix", &root, "pytest", 1, "AssertionError: expected 2 got 1");
    hooks::post_tool_use(&store, &failed).expect("record failure");
    let patch = json!({
        "session_id": "codefix",
        "tool_name": "apply_patch",
        "cwd": root.to_string_lossy(),
        "tool_input": {"command": "*** Begin Patch\n*** End Patch"},
        "tool_response": {"success": true}
    });
    hooks::post_tool_use(&store, &patch).expect("record patch success");
    let passed = bash_event("codefix", &root, "pytest", 0, "");
    hooks::post_tool_use(&store, &passed).expect("record test success");
    assert!(store.list_lessons(10).expect("lessons").is_empty());
}

// SPEC: sections 8, 13, operational-risk guidance
#[test]
fn stop_does_not_promote_unresolved_open_episode() {
    let (_temp, root, store) = temp_project();
    let failed = bash_event("stop-open", &root, "pytest", 1, "AssertionError");
    let first_output = hooks::post_tool_use(&store, &failed).expect("record failure");
    let case_id = first_output["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .expect("case context")
        .split("case ")
        .nth(1)
        .expect("case id suffix")
        .split('.')
        .next()
        .expect("case id")
        .to_string();
    hooks::stop(&store, &json!({})).expect("stop");
    let case = store
        .get_episode(&case_id)
        .expect("case lookup")
        .expect("case exists");
    assert_eq!(case.status.as_str(), "open");
}

// SPEC: sections 19, 20, operational-risk guidance
#[test]
fn purge_keeps_recent_attempts_and_reports_storage_stats() {
    let (_temp, root, store) = temp_project();
    for idx in 0..5 {
        let event = bash_event("purge", &root, &format!("echo {idx}"), 0, "");
        hooks::post_tool_use(&store, &event).expect("record attempt");
    }
    let before = store.stats().expect("stats before");
    assert_eq!(before.attempts, 5);
    let report = store.purge_keep_recent(2).expect("purge");
    assert_eq!(report.attempts_deleted, 3);
    let after = store.stats().expect("stats after");
    assert_eq!(after.attempts, 2);
    assert!(after.db_bytes > 0);
}

// SPEC: sections 19, 20, operational-risk guidance
#[test]
fn mark_lesson_result_updates_confidence_and_retirement() {
    let (_temp, root, store) = temp_project();
    let failed = bash_event("mark", &root, "npm test", 1, "use pnpm");
    let output = hooks::post_tool_use(&store, &failed).expect("record failure");
    let case_id = output["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .expect("case context")
        .split("case ")
        .nth(1)
        .expect("case id suffix")
        .split('.')
        .next()
        .expect("case id")
        .to_string();
    let registered = mcp::register_repair_episode(
        &store,
        RegisterRepairEpisodeInput {
            case_id: Some(case_id),
            reusable: true,
            failure_summary: "npm test failed".to_string(),
            repair_summary: "pnpm test passed".to_string(),
            lesson_hint: "Use `pnpm test` in this repo.".to_string(),
            trigger_description: "Running package test scripts.".to_string(),
            avoid_when: vec![],
            predicates: bash_predicates("npm", &["npm"]),
            scope: "project".to_string(),
            risk_level: "low".to_string(),
            confidence: 0.7,
        },
    )
    .expect("register");
    let lesson_id = registered["lesson_id"].as_str().expect("lesson id");
    store
        .mark_lesson_result(lesson_id, "confirmed")
        .expect("confirm");
    let confirmed = store
        .get_lesson(lesson_id)
        .expect("lesson")
        .expect("exists");
    assert_eq!(confirmed.status, "active");
    assert!(confirmed.confidence > 0.7);

    for _ in 0..3 {
        store
            .mark_lesson_result(lesson_id, "contradicted")
            .expect("contradict");
    }
    let retired = store
        .get_lesson(lesson_id)
        .expect("lesson")
        .expect("exists");
    assert_eq!(retired.status, "retired");
}

// SPEC: section 12, operational-risk guidance
#[test]
fn generated_ids_are_not_millisecond_process_local() {
    let first = domain::new_id("attempt");
    let second = domain::new_id("attempt");
    assert_ne!(first, second);
    assert!(first.split('_').count() >= 5, "{first}");
}

// SPEC: Demo D, section 12, operational-risk guidance
#[test]
fn redaction_covers_lowercase_bearer_headers_and_url_params() {
    let (_temp, root, store) = temp_project();
    let event = json!({
        "session_id": "secret-session-2",
        "tool_name": "Bash",
        "cwd": root.to_string_lossy(),
        "tool_input": {
            "command": "curl 'https://example.test?access_token=tok123&code=once456' -H 'x-api-key: raw789' -H 'authorization: bearer lower000'"
        },
        "tool_response": {
            "exit_code": 1,
            "stderr": "x-api-key: raw789\nhttps://example.test?access_token=tok123&code=once456\nauthorization: bearer lower000"
        }
    });
    hooks::post_tool_use(&store, &event).expect("record");
    let stored = serde_json::to_string(&store.list_attempts(10).expect("attempts")).expect("json");
    assert!(!stored.contains("tok123"), "{stored}");
    assert!(!stored.contains("once456"), "{stored}");
    assert!(!stored.contains("raw789"), "{stored}");
    assert!(!stored.contains("lower000"), "{stored}");
}

// SPEC: section 9.3
#[test]
fn project_hash_is_shared_by_repo_subdirectories() {
    let (_temp, root, _store) = temp_project();
    assert_eq!(
        domain::project_hash(&root.to_string_lossy()),
        domain::project_hash(&root.join("services/api").to_string_lossy())
    );
}
