# Agent memory

This file maps the project instructions that Codex and Claude Code share. It does not copy
those instructions because duplicate rules can drift.

## Canonical sources

- `AGENTS.md` contains the shared project model, working agreements, architecture rules,
  development commands, and current milestones.
- `CLAUDE.md` contains extended implementation notes for RAW, HEIF, Android, video, and
  platform decoders.
- `docs/backlog.md` contains open product work.
- `docs/audit/` contains the source-audit findings, split by area. Read
  `docs/audit/README.md` before you fix a defect.

## Agent integration

- Codex uses `.codex/config.toml` for project MCP servers.
- Codex discovers repository skills in `.agents/skills/`.
- Claude Code uses `.claude/` for local commands and skills.
- A reusable workflow must have a maintained version in `.agents/skills/`.

Update the canonical source when a decision changes. Do not copy the changed rule into this
index.
