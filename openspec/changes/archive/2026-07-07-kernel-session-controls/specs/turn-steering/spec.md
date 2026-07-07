# turn-steering

## ADDED Requirements

### Requirement: 流式回合中 Enter 即 steer
系统 SHALL 在 app-server 流式回合进行中(app_turn 活跃)、用户输入非空文本
并 Enter 时,发 `session/steer {sessionId, content}` 把输入注入当前回合,
输入以 User 条目落 transcript 并标注为转向;不再进入排队。steer 请求失败
(错误响应)时 SHALL 提示并把该输入退回排队,不中断当前回合。

#### Scenario: 中途转向
- **WHEN** 流式回合输出中,用户输入 "改用中文回答" 并 Enter
- **THEN** 发送 session/steer,输入落 transcript,当前回合继续(内核按 steer 语义处理),不排队不取消

#### Scenario: steer 失败退回排队
- **WHEN** steer 请求收到错误响应
- **THEN** 提示失败,该输入进入既有排队,回合不受影响

### Requirement: 非流式路径保留排队
系统 MUST 在 `--prompt` 路径的任务运行中、以及 app-server 的握手/drain
阶段,对输入 Enter 保持既有排队行为不变。

#### Scenario: 经典路径仍排队
- **WHEN** --prompt 任务运行中用户输入并 Enter
- **THEN** 显示 "queued (N waiting)",任务结束后按序提交,与本变更前一致
