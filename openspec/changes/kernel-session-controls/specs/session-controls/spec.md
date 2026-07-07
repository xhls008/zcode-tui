# session-controls

## ADDED Requirements

### Requirement: /model 切换
app-server 会话活跃时,`/model` SHALL 列出 `state.updated` 携带的
`model.available[]`(label + providerLabel)供 ↑↓/Enter 选择,选中后发
`session/setModel {sessionId, model: <available[].ref>}`;成功以
`mode_changed`/后续 state 推送回显为准,失败提示错误且不影响会话。
无 app-server 会话时 `/model` SHALL 提示该命令需要流式路径。

#### Scenario: 切换模型
- **WHEN** app-server 会话活跃,用户 /model 选中一个可用模型
- **THEN** 发送 setModel,状态栏/欢迎框回显新模型,后续回合用新模型

#### Scenario: 无会话时提示
- **WHEN** ZCODE_TUI_APP_SERVER 未设置或会话未建立,用户输入 /model
- **THEN** 显示需要 app-server 流式路径的提示,不发送任何请求

### Requirement: /think 思考级别
`/think` SHALL 在 `thoughtLevel.available` 值间切换(实测 enabled/disabled),
发 `session/setThoughtLevel {sessionId, ...}`,以状态推送回显;
失败提示且不影响会话。

#### Scenario: 切换思考级别
- **WHEN** app-server 会话活跃,用户 /think
- **THEN** 发送 setThoughtLevel 切到另一档,状态栏回显当前档位

### Requirement: /compact 上下文压缩
`/compact` SHALL 发 `session/compact {sessionId}`;压缩期间状态栏显示
compacting,完成后以状态推送刷新水位。≥80% 水位提示 MUST 同时给出
`/compact`(保会话)与 `/new`(丢会话)两个选项。

#### Scenario: 压缩降水位
- **WHEN** 上下文水位高,用户 /compact
- **THEN** 压缩完成后水位下降,会话继续可用(sessionId 不变)

#### Scenario: 高水位提示含 compact
- **WHEN** 水位 ≥80% 且 app-server 会话活跃
- **THEN** 提示文案含 /compact 与 /new 两个选项

### Requirement: /mode 走 setMode
app-server 会话活跃时,Shift+Tab 循环与 `/mode <m>` SHALL 发
`session/setMode {sessionId, mode}`(mode ∈ plan|build|edit|yolo|auto),
以 `reason:"mode_changed"` 推送回显;不再需要重启会话。无 app-server
会话时保持既有行为(记录到下次 spawn 的 CLI 参数)。

#### Scenario: 流式路径切模式即刻生效
- **WHEN** app-server 会话活跃,用户 Shift+Tab 切到 plan
- **THEN** 发送 setMode,mode_changed 推送到达后状态栏显示 plan,同一会话下一 prompt 受 plan 门禁

#### Scenario: 经典路径行为不变
- **WHEN** ZCODE_TUI_APP_SERVER 未设置,用户 Shift+Tab
- **THEN** 与本变更前一致:更新下次 --prompt 的 --mode 参数
