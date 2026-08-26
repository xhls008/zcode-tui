# Feature: feat-tool-output-clarity Clear tool output without manual folding

## Basic information
- ID: feat-tool-output-clarity
- Priority: 100
- Workflow mode: deep
- Risk signals: multi_module, compatibility, parallel_split
- Dependencies: none
- Plan phase: PR 1

## User outcome
The parent transcript stays concise without requiring `Ctrl+O`, while failures and every output explicitly requested by the user remain useful and complete.

## Scope and constraints

- Remove `Ctrl+O` folding state, overlay, index repair logic, help text, README text, and obsolete tests.
- Project common internal tools into structured summaries for Read, Search/Glob, Bash, Edit/Write, MCP, Subagent, and unknown tools.
- Include bounded diagnostic tails for failed tools.
- Preserve full assistant answers, `! command` output, `/diff`, `/usage`, `/status`, and other explicit user reports.
- Preserve inline viewport, terminal scrollback, resize, system selection, and copy behavior.
- Do not discard protocol payloads merely because the presentation is summarized.

## Acceptance scenarios

1. A long successful internal Read or Bash call produces a concise summary and does not require a fold control.
2. A failed Bash call shows exit status, duration when known, and a bounded useful stderr/compiler diagnostic tail.
3. `! command`, `/diff`, `/usage`, `/status`, and final assistant content remain complete.
4. Help, command hints, README files, and tests contain no `Ctrl+O` folding contract.
5. Resize, mouse scroll, terminal selection, and copy behavior match the pre-change behavior.

## Technical notes

Keep source classification explicit so an internal tool result cannot be confused with output the user requested directly. Summaries should be deterministic and independently unit tested.
