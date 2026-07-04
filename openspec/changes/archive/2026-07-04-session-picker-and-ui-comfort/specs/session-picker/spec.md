# session-picker

## ADDED Requirements

### Requirement: /sessions 会话选择浮层
The TUI SHALL provide a `/sessions` command that opens a floating picker
listing recent kernel sessions (read-only from the `session` table):
title (fallback: directory tail), shortened directory, relative age.
Sessions for the current directory MUST sort first, then by
`time_updated` descending, limited to 20 rows.

#### Scenario: 用户故事——找回昨天的会话
- **WHEN** 用户在项目目录打开 TUI,输入 `/sessions`,用 ↑↓ 选中昨天的会话并按 Enter
- **THEN** 浮层关闭,系统消息确认 `resuming sess_… on the next prompt`,下一条 prompt 带 `--resume <id>` 接续该会话

#### Scenario: Esc 放弃选择
- **WHEN** 浮层打开后按 Esc
- **THEN** 浮层关闭,会话状态与打开前完全一致

#### Scenario: db 降级时明确告知
- **WHEN** db 功能已降级(schema 不识别或库缺失),用户输入 `/sessions`
- **THEN** 显示一条系统消息说明会话列表不可用,不弹浮层、不报错刷屏
