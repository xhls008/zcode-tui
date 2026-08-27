# Feature: feat-glm-53-flash GLM-5.3-Flash runtime support

## Basic information
- ID: feat-glm-53-flash
- Priority: 90
- Workflow mode: deep
- Risk signals: compatibility, external_dependency
- Dependencies: none
- Created: 2026-08-27T01:28:37Z

## User outcome
Expose the Desktop-provided GLM-5.3-Flash model in the TUI and make new and resumed sessions use its valid runtime model metadata.

## Scope and constraints

- Read model metadata from the optional Desktop registry at `~/.zcode/v2/config.json`.
- Keep the CLI provider credentials and endpoint from `~/.zcode/cli/config.json`; never persist or log secrets.
- Merge Desktop model metadata only into the matching active CLI provider and preserve existing CLI models.
- Supply the enriched runtime model to both `session/create` and `session/resume`.
- Show enriched models in `/model` before and during a session.
- Treat missing, malformed, or unmatched Desktop state as a non-fatal fallback to the CLI catalog.
- Do not modify either user configuration file and do not push remote refs.

## Acceptance scenarios

1. With Desktop `builtin:bigmodel-coding-plan` metadata present, `/model` includes `GLM-5.3-Flash` under provider `bigmodel`.
2. The merged Flash entry retains its context window, output limit, and reasoning levels.
3. A new session receives the enriched `runtimeModel` and accepts `glm-5.3-flash` as a selectable model.
4. A resumed session receives the same enriched runtime model.
5. Existing CLI-only models remain available after merging.
6. Missing or malformed Desktop configuration leaves the CLI-only behavior working.
7. No configuration file is changed and no API key is emitted in verification output.

## Technical notes

- Desktop provider IDs use names such as `builtin:bigmodel-coding-plan`; CLI runtime provider IDs use `bigmodel`.
- Desktop model keys are display-oriented (`GLM-5.3-Flash`); runtime model IDs follow the CLI lowercase convention (`glm-5.3-flash`).
- Real protocol verification should use an isolated `zcode app-server` process and inspect only redacted model identifiers/metadata.
