# streaming-mcp-servers

## ADDED Requirements

### Requirement: 流式会话携带 MCP 配置
`session/create` 与 `session/resume` SHALL 附上由项目级 `.mcp.json` 与
用户级 `~/.config/zcode/mcp.json` 构造的 `mcpServers[]`(内核 schema
`$xe`;内核自身不读项目 `.mcp.json`,2026-07-07 实测钉死):stdio 服务器
为 `{name, command, args, env:[{name,value}]}`(无 type 字段),远程为
`{name, type:"http"|"sse", url, headers:[{name,value}]}`。`disabled`
的服务器 MUST 跳过;同名时项目级优先;两级均空时 MUST 不带 mcpServers
键。配置读取失败按既有宽松语义回空,MUST NOT 阻塞握手。

#### Scenario: 项目 .mcp.json 在流式会话生效
- **WHEN** cwd 的 .mcp.json 配了 stdio 服务器 zq-spike,首条流式 prompt 触发 session/create
- **THEN** create params 带 `mcpServers:[{name:"zq-spike", command, args, env:[]}]`,该会话内模型可见 `mcp__zq-spike__<tool>` 工具

#### Scenario: disabled 服务器不上车
- **WHEN** 某服务器在 .mcp.json 中 `"disabled": true`
- **THEN** mcpServers 数组不含它

#### Scenario: 无配置时形状不变
- **WHEN** 两级 MCP 配置都不存在或为空
- **THEN** create/resume params 与本变更前逐字节等价(不带 mcpServers 键)
