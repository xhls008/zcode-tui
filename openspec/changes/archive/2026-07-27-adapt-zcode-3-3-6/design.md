# adapt-zcode-3-3-6 · design

## Architecture

The adaptation has two independent seams:

1. **Kernel package resolution**: pure Rust helpers resolve and version-sort
   candidate application directories for startup/update reporting. The bash
   wrapper uses the same numeric ordering and exports the chosen `ZCODE_APP`
   whenever it hands control to the fallback TUI.
2. **Session tool policy**: CLI parsing stores normalized allow/deny entries in
   `AppConfig`. Classic prompts emit the kernel CLI flags; app-server
   create/resume attaches the verified strict protocol fields. Empty policy
   lists leave existing request shapes byte-identical.

## Interfaces

- `ZCODE_APP`: explicit active application directory; wrapper now supplies it
  to the fallback process.
- `ZCODE_TUI_UPDATE_FEED`: explicit feed URL or feed base. An explicit value is
  authoritative, including loopback URLs used by tests.
- `--allowed-tools <tools...>`: session tool allowlist.
- `--disallowed-tools <tools...>` / `--disallowedTools <tools...>`: session tool
  denylist.
- `--permission-mode <mode>`: legacy alias mapped to TUI `--mode`; `default`
  maps to the kernel's default permission mode, `build`.
- Protocol additions verified in the 3.3.6 bundle:
  `session/create {toolAllowlist?, toolDenylist?}` and
  `session/resume {toolAllowlist?, toolDenylist?}`.

Tool values are collected as shell arguments and split on commas only; internal
spaces are preserved so expressions such as `Bash(git *)` remain one rule.

## Data Model

`AppConfig` gains:

- `tool_allowlist: Vec<String>`
- `tool_denylist: Vec<String>`

Update resolution returns an application directory, an installed desktop
version (active rootless path first, dpkg fallback for `/opt/ZCode`), and a
normalized feed URL.

## Risks

- **Old strict app-server rejects new fields**: fields are sent only when the
  user supplied a policy. Existing downgrade discipline retries the prompt via
  the classic CLI; no-policy sessions remain byte-identical.
- **Development feed mistaken for official feed**: only implicit loopback URLs
  are replaced. `ZCODE_TUI_UPDATE_FEED` preserves intentional local feeds.
- **Version-like directory names contain suffixes**: numeric segment comparison
  matches the existing `is_newer_version` behavior and is covered with
  `3.10.0 > 3.9.9` tests.
- **Shell argument injection in `/update`**: feed and installed version are
  shell-quoted before interpolation; downloaded filenames still pass through
  `basename` and sha512 verification.

## Alternatives

- **Trust `app-update.yml` verbatim**: rejected because the observed 3.3.6
  artifact points at localhost and would silently break the documented update
  channel.
- **Hard-code only the official CDN**: rejected because explicit local feeds
  are valuable for deterministic smoke tests and alternate official mirrors.
- **Pass all newly advertised flags to app-server**: rejected because the
  strict 3.3.6 schemas expose tool lists but not `settings` or `maxTurns`.
