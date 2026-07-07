# streaming-prompt Specification

## Purpose
TBD - created by syncing change app-server-streaming. Update Purpose after archive.
## Requirements
### Requirement: 试验开关下的真流式正文
`ZCODE_TUI_APP_SERVER=1` 时,prompt 提交 SHALL 走 app-server:助手正文按
`state.updated` 修订补丁**增量实时渲染**进 transcript(单轮纯问答亦然,
真 token 级流式);开关未设时 MUST 完全走现有 `--prompt` 路径,行为不变。

#### Scenario: 单轮问答流式
- **WHEN** ZCODE_TUI_APP_SERVER=1,提交一个不调工具的纯文本 prompt
- **THEN** 助手正文在生成过程中逐步出现在 transcript,而非结束时整块

#### Scenario: 开关关闭行为不变
- **WHEN** ZCODE_TUI_APP_SERVER 未设置
- **THEN** prompt 走 --prompt,与本变更前完全一致

### Requirement: 失败无缝降级
系统 MUST 在 app-server 路径任一环节失败(spawn、连接、schema 不符、
send 被拒、连接中断)时无缝降级回 `--prompt`:本进程永久改走 --prompt,
当前这条 prompt 用 --prompt 重试一次,用户不卡死;降级时 MUST 记一条
dim 系统提示。

#### Scenario: 运行中连接断开
- **WHEN** 流式过程中 app-server 连接中断
- **THEN** 当前 prompt 自动改用 --prompt 完成,后续 prompt 也走 --prompt,一条 dim 提示说明已降级

#### Scenario: 握手即失败
- **WHEN** 开关开启但 app-server 起不动
- **THEN** 首条 prompt 直接走 --prompt,不显示半截流式,一条 dim 提示

### Requirement: 流式路径的权威进度
app-server 路径下,工具调用状态、上下文水位 SHALL 取自 `state.updated`
权威事件而非 db 轮询;取消经 `session/stop`。db 轮询仅在 --prompt 路径继续。

#### Scenario: 工具与水位来自协议
- **WHEN** 流式路径运行一个调用工具的 prompt
- **THEN** 工具 chip 与上下文水位来自协议事件,不依赖 db.sqlite 轮询
