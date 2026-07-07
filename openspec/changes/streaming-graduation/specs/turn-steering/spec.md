# turn-steering

## MODIFIED Requirements

### Requirement: 流式回合中 Enter 即 steer
系统 SHALL 在 app-server 流式回合进行中(app_turn 活跃)、用户输入非空文本
并 Enter 时,发 `session/steer {sessionId, content}` 把输入注入当前回合,
输入以 User 条目落 transcript 并标注为转向;不再进入排队。steer 的响应
MUST 按两层判定:信封错误,或成功信封内 `result.kind == "rejected"`
(reason ∈ no_active_turn/expected_turn_mismatch/turn_not_steerable/
empty_input/input_too_large)——两者都 SHALL 按 reason 提示并把该输入退回
排队,不中断当前回合;`result.kind == "queued"` 才算注入成功。

#### Scenario: 中途转向
- **WHEN** 流式回合输出中,用户输入 "改用中文回答" 并 Enter
- **THEN** 发送 session/steer,结果 kind=queued,输入落 transcript,当前回合继续,不排队不取消

#### Scenario: steer 失败退回排队
- **WHEN** steer 请求收到错误响应
- **THEN** 提示失败,该输入进入既有排队,回合不受影响

#### Scenario: steer 被拒退回排队
- **WHEN** steer 成功响应但 result.kind=rejected(如 turn_not_steerable)
- **THEN** 按 reason 提示,该输入退回排队,回合结束后按序作为新 prompt 提交
