# persistent-history

## ADDED Requirements

### Requirement: 内核历史读入
At startup (db enabled) the TUI SHALL load up to 200 entries from the
kernel's `input_history` table (oldest→newest) as the base of the Up/Down
input history; entries submitted in this process append on top. Adjacent
duplicates MUST be collapsed. The TUI MUST NOT write to the table.

#### Scenario: 用户故事——上周的命令还能按上箭头找到
- **WHEN** 用户重启 TUI 后按 Up
- **THEN** 依次回到之前进程里提交过的输入(来自内核 input_history),而不是空历史

#### Scenario: db 降级时退回进程内历史
- **WHEN** db 功能降级
- **THEN** Up/Down 仅含本进程历史,无错误提示

### Requirement: Ctrl+R 反向搜索
`Ctrl+R` SHALL open a reverse-search overlay over the merged history:
typing filters by substring (newest first), ↑↓ moves the selection,
Enter puts the selected entry into the composer, Esc cancels and leaves
the composer untouched. The matcher MUST be a pure lib.rs function.

#### Scenario: 用户故事——只记得命令里有个词
- **WHEN** 用户按 Ctrl+R 输入 `mcp`
- **THEN** 浮层按新→旧列出所有含 `mcp` 的历史输入,Enter 取回选中项到输入框
