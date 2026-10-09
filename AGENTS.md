# AGENTS.md
Auto-loaded by opencode at the start of every session. Read this first to understand the project, its conventions, and its workflow.

## Goal
convert [text](../DigitalTwinDashboard) to a rust based app.

## Workflow
`AGENTS.md` -> project context, workflow rules (this file).
`SESSION.md` ->  Session history, usually asked by the user before closing the session. *timeline: top entry is newest*
`HANDOFF.md` -> known-issues, next-steps. Current state of the project, future scope
`LOG.md` -> Per-chat log. **Heading after every chat**. Full narrative lives in `SESSION.md`. *timeline: top entry is newest*

## Rules
*follow strictly every session*

1. **Backup before writing** Before modifying files, copy the ones you're about to change into `backups/YYYY-MM-DD-time-topic/`
2. **At session start:** read `HANDOFF.md` (pick up where work left off) and `SESSION.md` (what has been done before, top entry is newest) if available otherwise create
3. **`HANDOFF.md` is updated constantly.** Whenever something relevant changes mid-session (a bug found, a decision made, a next step shifting), update `HANDOFF.md` immediately.
4. **When the user ends a session and asks to save it**, write the full session summary into `SESSION.md`. At that same moment, **verify/trim `HANDOFF.md` against the session**: move completed items into the SESSION md entry and keep only current, actionable next-steps.
6. **Token efficiency** Save token usasge without sacrificing **performance**.
7. **Unchanged translation** nothing (*features and functionality*) from the original version could vanish, it should be as 1:1 ratio translastion possible.
8. **Avoid Ai-detection** dont spam unneccessary m-dash (—), ai language/phrasing (not this, that. 3 things again and again, etc) , emojis — like ai
9. **Dont frezz** check every 3 min if you are stucked in a proccess or not.
10. **User can be wrong** im a an ameture so i could be wrong in most instances so push back if you feels like.
11. **Let user go AFK** at the beginning of the task when starting to complete to do list ask for all the permission at once so user can allow or reject and go take a break

## Project Overview

1:1 Rust port of `D:/Git/DigitalTwinDashboard` (read-only original):
`ESP32 (Arduino C++) --MQTT--> Python FastAPI backend --WS--> vanilla HTML/CSS/JS dashboard`.
Rust keeps every feature (rule 7). Strategy: **Phase A** = Axum backend serving
existing pages untouched (guaranteed 1:1); **Phase B (opt-in)** = Leptos/Yew
rewrite page-by-page with vanilla fallback. Firmware stays C++ (contract only).

Skills (`.opencode/skills/*/SKILL.md`): `rust-axum-backend`, `rust-mqtt-ingest`,
`rust-telemetry-core`, `rust-excel-export`, `leptos-yew-frontend` (Phase B only),
`firmware-contract-guard`, `windows-rust-ops`, `project-workflow`, `rust-parity-test`.
