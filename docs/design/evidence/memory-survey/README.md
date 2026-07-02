# Memory systems survey — evidence archive

Generated 2026-07-01 by a multi-agent code survey (7 deep-readers + 6 adversarial
follow-up investigations) over five codebases:

- Claude Code (`~/src/cc/claude-code`) — auto-memory/memdir, session memory,
  extraction, autoDream, team sync, CLAUDE.md hierarchy
- OpenAI Codex CLI (`~/src/cc/codex`) — memories pipeline, rollouts,
  message-history, AGENTS.md
- Meerkat (`~/src/meerkat`) — meerkat-memory crate + runtime wiring
- MobKit (this repo, `main` at 65be6546) — agent_memory, operational ledger,
  meerkat delegation
- Elephant (`~/src/elephant`) — full pipeline, storage, truth maintenance,
  MCP/policy/identity surfaces

`reports.md` holds the seven structured system reports. `followups.md` holds the
six verified follow-up investigations (echo loops, lifecycle orphaning, injection
duplication, Elephant TM mechanics, Codex crash semantics, markdown-store
supersede absence).

File:line citations reflect the surveyed checkouts on 2026-07-01; verify against
current code before relying on them.
