# prompt-json-result Specification

## Purpose
TBD - created by archiving change db-live-progress-and-auth-screen. Update Purpose after archive.
## Requirements
### Requirement: prompt 通道使用 --json 并解析总结对象
TUI SHALL 在 prompt 调用上附加 `--json`,并将结尾整块输出解析为总结
对象,提取 `response`(作为助手回复走 markdown 渲染)、`sessionId`、
`usage`、`projection.contextUsed/contextWindow`。

#### Scenario: 正常总结对象
- **WHEN** prompt 结束,stdout 为含 response/sessionId/usage/projection 的 JSON 对象
- **THEN** response 以 markdown 渲染入 transcript,sessionId 更新会话状态,水位数据入状态栏

#### Scenario: 解析失败降级
- **WHEN** stdout 不是可解析的总结对象(内核未来改格式或输出纯文本)
- **THEN** 整个输出按现有纯文本/markdown 路径渲染,会话与水位状态保持原值,不报错

### Requirement: 状态栏上下文水位
拿到 projection 后,状态栏 SHALL 显示上下文水位(如 `ctx 9k/200k` 或
百分比),仅在数据可用时显示;水位过高(≥80%)时 SHALL 以 dim 提示
建议 `/new`。

#### Scenario: 水位显示与提醒
- **WHEN** contextUsed/contextWindow = 170000/200000
- **THEN** 状态栏显示水位且附 dim 的 /new 建议;低水位时无建议文案

