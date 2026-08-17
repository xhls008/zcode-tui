# adapt-zcode-3-7-6 · design

## Context

Desktop `3.7.6` introduced the change on 2026-08-10. The current 2026-08-14
official x64 feed resolves to `3.7.7`; its deb reports `3.7.7-4926` and matches
the feed SHA-512. Both CLIs report `0.16.3` and cannot resolve `@zcode/tui`.

Live probing shows `session/create` first emits:

```json
{"id":"server-1","method":"session/requestRuntimePreferences","params":{"sessionId":"sess_...","scope":"runtime-materialization"}}
```

The response schema is strict:

```json
{
  "nativeSearchEnhancementsEnabled": true,
  "memoryEnabled": false,
  "askUserQuestionAutoResolutionEnabled": true,
  "modelContextBudgetStrategy": "preflight-v1"
}
```

The same method may later request scope `user-execution`, optionally allowing
an `integratedTerminalShell`. Omitting that optional field lets the kernel
select the host shell normally.

## Architecture

Add one pure encoder that recognizes the runtime-preferences method and returns
the complete reply envelope with the original string-or-number id. Dispatch it
before interactive approval parsing in the existing server-request handler.

The handler is shared by handshake, active-turn, and idle pumps. The handshake
pump must explicitly dispatch server requests instead of dropping every
non-response message while waiting for `session/create`/`resume`.

## Interfaces

- Method: `session/requestRuntimePreferences`
- Params consumed: only the method identity; `sessionId` and `scope` are not
  echoed and are safe to ignore for the compatibility defaults.
- Result: four required fields above; no extra keys.
- Envelope id: echoed verbatim, including values such as `"server-1"`.

## Risks

- **Strict result schema**: construct the exact known object and unit-test no
  extra keys.
- **Handshake-only fix misses later requests**: route through the shared
  `on_server_request` handler used during handshake, turns, and idle state.
- **Old-kernel regression**: 0.15.x never sends this method, so its byte-level
  request flow remains unchanged.

## Alternatives

- Return Method not found: the 0.16.3 kernel has a fallback for that error, but
  the current connection API only sends success replies and using defaults
  directly is smaller and avoids intentional protocol errors.
- Disable app-server on 3.7.6+: rejected because the required response is small
  and all previously verified legacy/V4 methods remain present.
- Implement configurable memory/shell preferences: rejected as unrelated UI
  scope and unnecessary for restoring streaming.
