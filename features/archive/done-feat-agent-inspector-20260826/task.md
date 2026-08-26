# Tasks: feat-agent-inspector

- [x] Define list/detail view state for Agents and Background tabs.
- [x] Render parent, Subagent, task status, summaries, links, and available output tails.
- [x] Implement navigation, refresh, selection preservation, and detail scrolling.
- [x] Make read-only viewing and parent input targeting explicit in every relevant state.
- [x] Add reducer/render tests plus interactive narrow/wide terminal acceptance checks.

## Progress log

- 2026-08-26: Existing `/agents` lifecycle overlay recorded as the compatibility baseline.
- 2026-08-26: Added Agents/Background tabs, list/detail navigation, stable keyed selection, and detail scrolling.
- 2026-08-26: Added parent-target/read-only messaging, official output tails, linked work, refresh, and copy-leader compatibility.
- 2026-08-26: Passed wide/narrow render tests, strict linting, 127 tests, and release build.
