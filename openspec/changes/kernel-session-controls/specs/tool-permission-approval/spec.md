# tool-permission-approval

## ADDED Requirements

### Requirement: 服务器→客户端交互请求的解码
解码层 MUST 把 `{id, method, params}` 且 `method` 与 `id` 同时存在的行识别
为**服务器→客户端请求**(新消息类别),`id` MUST 兼容字符串(实测为
`"server-1"`、`"server-2"`……)与数字;不得再被当作无法解析的行忽略,
也不得与客户端请求的 Response(数字 id)混淆。解码为 lib.rs 纯函数,可单测。

#### Scenario: 字符串 id 的交互请求被识别
- **WHEN** 收到 `{"id":"server-1","method":"interaction/requestUserInput","params":{...}}`
- **THEN** 解码为服务器请求变体,携带信封 id、method 与 params,而非被忽略或误判为 Response

#### Scenario: 数字 id 的响应不受影响
- **WHEN** 收到 `{"id":3,"result":{...}}`
- **THEN** 仍解码为 Response 并按 u64 id 关联,行为与本变更前一致

### Requirement: 确认浮层与应答
TUI SHALL 在 `interaction/requestUserInput` 到达时(params 含 `prompt`、
`questions[{header, question, options[{label, value, description}]}]`、
`requestId`、`toolName`)弹确认浮层显示 prompt/question 与选项
(↑↓ 选择、Enter 确认、Esc 拒绝),并以
`{"id":<信封id>,"result":{"requestId":…,"answers":{<header>:<option value>}}}`
应答;应答后浮层关闭,回合继续由既有事件流驱动。

#### Scenario: 批准
- **WHEN** plan 模式下被门禁的工具触发交互请求,用户 Enter 选中 "Approve"
- **THEN** 发送 answers={header:"approve"} 应答,内核停止重发,回合正常收尾(不再挂到 600s 兜底)

#### Scenario: 拒绝
- **WHEN** 交互浮层打开,用户按 Esc
- **THEN** 以非批准语义应答(实现期 spike 钉死具体值;若协议无拒绝值则以关闭浮层+session/stop 兜底),回合不挂起

### Requirement: 重发去重
TUI MUST 按 `requestId` 去重——内核会对同一 `requestId` 以新信封 id 退避
重发(实测 ~1s/2s/4s/8s/10s…):同一 requestId 只弹一次浮层,重发到达时仅
更新待应答的信封 id(应答须用最新信封 id 或任一有效信封 id——实现期以
spike 验证哪种被接受);已应答的 requestId 的迟到重发 MUST 安静丢弃。

#### Scenario: 重发不闪屏
- **WHEN** 同一 requestId 的请求以信封 id server-1…server-4 到达 4 次
- **THEN** 浮层只弹一次,内容不闪烁重建

#### Scenario: 迟到重发被丢弃
- **WHEN** 用户已应答,之后又收到同一 requestId 的重发
- **THEN** 安静忽略,不再弹层不再应答

### Requirement: 非 app-server 路径不受影响
交互请求处理 MUST 仅存在于 app-server 路径;`--prompt` 路径(含降级后)
行为与本变更前完全一致。

#### Scenario: --prompt 路径无浮层
- **WHEN** ZCODE_TUI_APP_SERVER 未设置,plan 模式提交 prompt
- **THEN** 走既有 --prompt 路径,无任何交互浮层逻辑参与
