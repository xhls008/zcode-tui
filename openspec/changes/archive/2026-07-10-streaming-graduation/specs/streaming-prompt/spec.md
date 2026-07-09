# streaming-prompt

## MODIFIED Requirements

### Requirement: 试验开关下的真流式正文
系统 SHALL 默认走 app-server 流式路径:prompt 提交经
`session/create/resume → subscribe → send`,助手正文逐 token 增量渲染进
transcript(单轮纯问答亦然)。`ZCODE_TUI_APP_SERVER=0/off/false` 时 MUST
完全走经典 `--prompt` 路径,行为与流式引入前一致;`=1/true/on` 显式开启
(与默认等效,保持向后兼容)。

#### Scenario: 默认即流式
- **WHEN** 未设置 ZCODE_TUI_APP_SERVER,提交一个纯文本 prompt
- **THEN** 助手正文在生成过程中逐步出现在 transcript,而非结束时整块

#### Scenario: 显式关闭走经典路径
- **WHEN** ZCODE_TUI_APP_SERVER=0
- **THEN** prompt 走 --prompt,与流式路径引入前完全一致

#### Scenario: 显式开启仍有效
- **WHEN** ZCODE_TUI_APP_SERVER=1(既有脚本/wrapper 写法)
- **THEN** 行为与默认一致,不告警不报错
