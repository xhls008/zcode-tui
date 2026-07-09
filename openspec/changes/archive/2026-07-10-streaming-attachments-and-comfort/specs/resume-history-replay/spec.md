# resume-history-replay

## ADDED Requirements

### Requirement: resume 历史紧凑回放
流式 `session/resume` 成功后,TUI SHALL 把结果 `messages[]`(实测形状
`{info:{role}, parts:[{type:"text", text}…]}`)中末尾至多 6 条
role∈{user,assistant} 且拼接文本非空的消息,以 dim 紧凑形式渲染进
transcript(user 与 assistant 有可区分前缀),每条预览按 ~400 字符截断
加省略号;既有 "resumed sess_…(N messages)" 提示保留为回放小节头。
messages 缺失或没有可回放条目时 MUST 只保留小节头(行为与现状一致)。

#### Scenario: 续接后看到上下文
- **WHEN** /sessions 选中一个有历史的会话并 resume 成功
- **THEN** transcript 先是 "resumed sess_…(N messages)" 头,随后 ≤6 条 dim 预览(旧→新),每条 ≤400 字符

#### Scenario: 超长消息截断
- **WHEN** 历史里某条 assistant 回复长于 400 字符
- **THEN** 预览截断到 ~400 字符并以 "…" 结尾,不撑爆屏幕

#### Scenario: 非文本消息跳过
- **WHEN** 某条消息 parts 只有 file/reasoning/step-* 等非 text 部件
- **THEN** 该条不进回放,不产生空行
