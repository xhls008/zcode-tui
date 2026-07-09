# session-controls

## ADDED Requirements

### Requirement: 状态栏模型与模式显示
footer 右侧 SHALL 在上下文水位与认证标签之外常驻显示当前模型与权限模式
(形如 `glm-5.1 · build`):模型取 SessionControls 缓存的
`model_current`,模式取缓存的 `mode`;内核首次 state 推送到达前 SHALL
回退显示 config 的模式(模型未知则只显示模式)。显示 MUST 随
`mode_changed` 等状态推送更新(不乐观更新)。

#### Scenario: 流式会话显示模型与模式
- **WHEN** 一条流式 prompt 完成,内核推送过 model.current 与 mode.current
- **THEN** footer 右侧显示 `glm-5.1 · build`(值来自推送缓存)

#### Scenario: 首推送前回退
- **WHEN** 启动后尚无任何 state 推送,--mode plan 生效
- **THEN** footer 右侧显示 `plan`(无模型段),不显示过期或猜测的模型名
