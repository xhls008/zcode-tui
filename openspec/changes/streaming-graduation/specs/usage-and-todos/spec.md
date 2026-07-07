# usage-and-todos

## ADDED Requirements

### Requirement: /usage 用量统计
`/usage` SHALL 显示会话级与周期用量:app-server 会话活跃时发
`session/usage {sessionId}`(totalTokens/input/output/reasoning/请求数)与
`usage/stats {range:"7d"}`(汇总含 cacheHitRate、totalSessions),渲染为
System 条目;`/usage 30d` 切换周期(值域 7d|30d)。无 app-server 会话时
提示需要流式路径。

#### Scenario: 查看用量
- **WHEN** app-server 会话活跃,用户 /usage
- **THEN** transcript 显示本会话 token 细分与 7 天汇总(含缓存命中率)

### Requirement: TODO 清单显示
系统 SHALL 把 create/resume 结果与 state 推送携带的 todos(及 todoGroups)
渲染进运行工作区:有未完成项时显示紧凑清单(状态符号 + 文本),空列表不
占屏;清单随推送更新。

#### Scenario: 内核任务清单可见
- **WHEN** 内核在回合中维护 todos(agentic 任务)
- **THEN** 工作区显示清单及各项状态,完成项打勾,回合结束后清单保留至下次更新

### Requirement: 内核 slashCommands 并入补全
`/` 补全 SHALL 合并 create/resume 结果的
`slashCommands[]{name, description, inputHint}`:本地实现的命令优先且
同名去重,内核命令标注来源并按 inputHint 展示;这些命令提交时按既有
路由转发内核(prompt 通道)。

#### Scenario: 内核命令可补全
- **WHEN** 会话建立后用户输入 "/go"
- **THEN** 补全列表含内核上报的 /goal(带 inputHint),选中提交后转发内核
