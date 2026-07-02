# Agent TUI Usage Habits Reference

Captured on 2026-07-02 for `zcode-tui` design work.

## Local Raw Snapshots

- `docs/references/raw/claude-code-interactive-mode.html`
- `docs/references/raw/claude-code-slash-commands.html`
- `docs/references/raw/claude-code-settings.html`
- `docs/references/raw/claude-code-llms-full.txt`
- `docs/references/raw/opencode-tui.html`
- `docs/references/raw/opencode-keybinds.html`

OpenCode does not currently expose `llms.txt` or `llms-full.txt` at the checked paths; those requests returned 404.
Placeholder GitHub token examples in the Claude Code LLM index snapshot were redacted before publishing so the public repository does not trip secret scanners.

## Source URLs

- Claude Code interactive mode: https://code.claude.com/docs/en/interactive-mode
- Claude Code slash commands: https://code.claude.com/docs/en/slash-commands
- Claude Code settings: https://code.claude.com/docs/en/settings
- Claude Code LLM index: https://code.claude.com/docs/llms-full.txt
- OpenCode TUI: https://opencode.ai/docs/tui/
- OpenCode keybinds: https://opencode.ai/docs/keybinds/

## Claude Code Habits To Borrow

- Keep a persistent bottom input and send with Enter.
- Make slash commands discoverable through `/help`, command lists, and prefix filtering.
- Treat `! <command>` as a shell escape for quick local commands.
- Use `@...` mentions as a mental model for file, agent, or repo targeting; this fallback can show the habit even if ZCode owns the actual interpretation.
- Keep obvious scrollback controls: PgUp, PgDn, Home, End.
- Use context-sensitive help with `?` or `/help`.
- Surface session status, current working directory, mode/model-like state, and command route.
- Support editor handoff through `$VISUAL` or `$EDITOR` for longer prompts.
- Keep MCP and settings visible because agent tools depend heavily on configuration.

## OpenCode Habits To Borrow

- Provide a command palette habit on `Ctrl+P`.
- Use a leader-key model for discoverable power-user shortcuts.
- Let `keybinds` and TUI behavior be configurable over time; this implementation starts with built-in defaults.
- Make theme a first-class concept. OpenCode documents a `theme` field and command palette settings; this fallback should expose a strong default style.
- Use `$EDITOR` for `/editor` style workflows and document that GUI editors should use blocking flags such as `--wait`.
- Keep scroll speed and attention/status events visible in the UI.

## zcode-tui Decisions

- `Ctrl+P`: command palette.
- `Ctrl+X`: leader prefix, then `p` palette, `h` help, `e` editor, `x` clear conversation, `u` clear input, `q` quit.
- `Tab`: complete or show slash command suggestions when input starts with `/`.
- `! <cmd>`: local shell command routed through `sh -lc`.
- `Ctrl+G` or `/editor`: open the current prompt in `$VISUAL` or `$EDITOR`.
- `Ctrl+J`: insert newline in the prompt box.
- Visual direction: dense terminal instrument panel, neon cyan/green/magenta, route labels, signal/status bars, no soft card UI.
