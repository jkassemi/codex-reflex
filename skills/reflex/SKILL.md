---
name: reflex
description: Use after failed supported Codex tool calls are repaired to register concise, scoped operational lessons with the Reflex MCP server.
---

# Reflex

Use Reflex to preserve reusable operational repairs from failed tool calls.

When a tool call fails, do not immediately record a lesson. Continue diagnosing normally.

If Reflex blocks a tool call and supplies the replacement command, use that command without registering a new lesson for the same repair.

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
- broad rules like "always use sudo"
- ordinary code/test failures whose fix was changing product code
- repairs Reflex already supplied through a blocking hook response
- guesses that have not succeeded

Keep lessons scoped, concise, and falsifiable.
