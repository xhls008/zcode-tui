# live-progress Specification

## Purpose
TBD - created by archiving change db-live-progress-and-auth-screen. Update Purpose after archive.
## Requirements
### Requirement: 运行中工具进度展示
prompt 任务运行期间,TUI SHALL 以 ~400ms 节奏(主循环分频)轮询 db
增量,将工具调用渲染为 chip:运行中显示 spinner 与工具名,完成显示
`✓` 与耗时,失败显示 `✗`(语义红)。db 功能降级时 MUST 回到现有的
纯 spinner 行为。

#### Scenario: 工具边跑边显示
- **WHEN** prompt 运行中,内核先后执行 Read 与 Bash 两个工具
- **THEN** 工作区先出现 `⚙ Read`(运行中),完成后变 `✓`,随后出现 `⚙ Bash`,全程无需等 turn 结束

#### Scenario: 轮询一拍失败
- **WHEN** 某拍 db 查询抛 busy 或 IO 错误
- **THEN** 该拍跳过,已显示的 chip 保持原样,下一拍继续

### Requirement: reasoning 仅运行时显示
运行期间 TUI SHALL 在 spinner 工作行下方以 dim 样式显示最新 reasoning
片段(单行,超宽截断);turn 结束后该内容 MUST 随工作区整体消失,
MUST NOT 写入 transcript。

#### Scenario: reasoning 片段滚动更新
- **WHEN** 运行中出现多个 reasoning part
- **THEN** 工作行始终只显示最新一条的首行,dim 样式

#### Scenario: turn 结束清场
- **WHEN** prompt 任务收尾(Finished + Eof 齐)
- **THEN** 工具 chip 与 reasoning 行全部消失,transcript 只追加权威结果单元

### Requirement: 运行时进度不污染权威结果
轮询获得的文本内容 MUST NOT 作为 transcript 单元写入;transcript 的
助手回复 MUST 仅来自 stdout 权威结果(见 prompt-json-result)。

#### Scenario: 进度与结果不重复
- **WHEN** 运行中 part 已显示过工具与 reasoning,turn 结束 stdout 返回最终 response
- **THEN** transcript 中该回复只出现一次(来自 stdout),无轮询内容残留

