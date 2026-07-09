# clipboard-copy

## ADDED Requirements

### Requirement: OSC52 复制最后回复
`Ctrl+X` 后 `y`(leader 表)与 `/copy` 本地命令 SHALL 把 transcript 中
最后一条助手回复经 OSC52 序列(`ESC ] 52 ; c ; <base64> BEL`)写到
TUI 拥有的 stdout(裸终端),由终端落系统剪贴板;base64 负载 MUST 以
~100KB 为上限,超限按源文本边界截断后再编码(序列必须始终合法)。
无助手回复时 MUST 提示而不发序列。tmux 场景 SHALL 由 README 引导
`set -g set-clipboard on`,TUI 不做 passthrough 包裹。

#### Scenario: 复制到系统剪贴板
- **WHEN** 一轮回答完成后按 Ctrl+X y(或输入 /copy)
- **THEN** stdout 收到一条合法 OSC52 序列,内容为最后一条助手回复的 base64;状态栏回显 copied

#### Scenario: 超长回复截断
- **WHEN** 最后一条助手回复 base64 后超过 100KB
- **THEN** 按上限截断源文本后编码,序列合法、剪贴板拿到前缀

#### Scenario: 无可复制内容
- **WHEN** 会话里还没有任何助手回复时触发复制
- **THEN** 状态栏提示无内容,不向终端写 OSC52 序列
