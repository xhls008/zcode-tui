# turn-steering Specification (delta)

## MODIFIED Requirements

### Requirement: 流式回合中 Enter 即 steer

系统 SHALL 在 app-server 流式回合进行中、用户输入非空文本并 Enter 时按
协商能力执行中途转向：V4 可用时先接受 `setFollowupMode {mode:"guide"}`，
再发 V4 `sendText`；仅当 direct ack 或随后 V4 frame 的 admitted delivery
为 `guide` 时才能把输入标注为已转向。V4 不可用
的旧内核 SHALL 使用 `session/steer {sessionId,content}`。任何失败、
`delivery:"queue"` 或 `delivery:"startNow"` 都 MUST 如实提示并按内核实际
处置更新 UI，不得仅凭本地 optimistic marker 宣称 steer 成功。

#### Acceptance Criteria

- Given 3.5.3 V4 状态可用且回合进行中, when 用户输入“改用中文回答”, then
  先接受 guide 模式，再发送文本，ack delivery 为 guide，transcript 才标注
  已转向。
- Given 3.5.3 send ack delivery 为 queue, when 处理回执, then UI 显示排队
  而不是转向成功，不再额外本地重复排队同一文本。
- Given V4 send 被拒或 CAS 陈旧, when 处理回执, then 显示失败、文本只回到
  本地队列一次，当前回合不中断。
- Given 3.3.6 不支持 V4, when 回合中 Enter, then 使用既有 session/steer；
  错误响应仍提示并退回排队。
- Given PTY 测试只看到了本地“steering”标记但随后 Method not found, when
  场景结束, then 测试 MUST 失败。

### Requirement: 非流式路径保留排队

系统 MUST 在普通经典 `--prompt` 任务运行中、Browser Use 强制经典路径中，
以及 app-server 握手/drain 阶段，对输入 Enter 保持既有排队行为，不尝试 V4
或 legacy steer。

#### Acceptance Criteria

- Given 普通或 Browser Use 的 `--prompt` 任务运行中, when 用户输入并 Enter,
  then 显示 queued，任务结束后按序提交。
