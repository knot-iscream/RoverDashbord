---
name: project-workflow
description: Use every session for backups, HANDOFF.md, LOG.md, SESSION.md, and 1:1 translation checks.
---

# Project Workflow

Enforces `AGENTS.md` rules 1-4+6-7.

## Rules
- Backup BEFORE writing: copy affected files to `backups/YYYY-MM-DD-HHmm-topic/` first.
- Session start: read `HANDOFF.md` + `SESSION.md` (top entry newest). Session end (only when user asks to save): append full summary to `SESSION.md`, trim `HANDOFF.md` to actionable next-steps only.
- After every chat: append one terse line to `LOG.md` (`YYYY-MM-DD — summary`). Never edit old lines.
- Token efficiency: targeted reads/grep over full files, parallel tool calls, never re-read unchanged files.
- 1:1 gate: every ported endpoint/WS message/payload field checked against the original (`backend/*.py`, `*.js`, `firmware/*.h`); nothing drops without user sign-off.
