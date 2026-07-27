# app-server-tool-policy Specification

## Purpose

Keep ZCode tool allow/deny policy and permission-mode arguments equivalent
across the classic prompt and app-server execution paths.

## Requirements

### Requirement: 工具白名单/黑名单跨两条执行路径一致

TUI MUST 接受 `--allowed-tools`、`--disallowed-tools` 与
`--disallowedTools`。经典路径 MUST 把策略传给 `zcode --prompt`；app-server
路径 MUST 在 `session/create` 和 `session/resume` 中分别使用 3.3.6 strict
schema 字段 `toolAllowlist[]` 与 `toolDenylist[]`。未提供策略时请求形状 MUST
与变更前完全一致。

#### Acceptance Criteria

- Given `--allowed-tools Read,Glob --disallowed-tools 'Bash(git *)'`, when
  新建 app-server 会话, then create params 含
  `toolAllowlist:["Read","Glob"]` 与 `toolDenylist:["Bash(git *)"]`。
- Given 同一参数并选择 resume, when 发起握手, then resume params 含相同策略。
- Given resume 失败并回退 create, when 发送第二个握手请求, then 策略不丢失。
- Given 没有任何策略参数, when 编码 create/resume, then 不出现两个新字段。

### Requirement: 工具规则解析保持表达式完整

逗号分隔的规则 MUST 拆为独立条目；每个 shell argv 内部的非首尾空格 MUST
保留，使 `Bash(git *)` 这类规则不会被错误拆分。空条目 MUST 丢弃。

#### Acceptance Criteria

- Given `Read,Glob`, when 解析, then 得到 `Read`、`Glob`。
- Given 单个 argv `Bash(git *)`, when 解析, then 得到一个完整规则而不是两个。

### Requirement: legacy permission-mode 别名

TUI MUST 把 `--permission-mode build|edit|plan|yolo` 当作 `--mode`；值
`default` MUST 映射为内核当前默认的 `build`。同一命令行多次指定 mode 时
MUST 以最后一次为准。

#### Acceptance Criteria

- Given `--permission-mode plan`, when 首次建立流式会话, then 既有 setMode
  路径把会话切到 plan。
- Given `--mode edit --permission-mode default`, when 解析, then 最终 mode 为
  build。

### Requirement: 不虚构未暴露的内核参数语义

对于 3.3.6 help 中出现但实际 CLI parser 拒绝、且 app-server schema 未暴露
对应字段的 `--settings` 与 `--max-turns`, TUI MUST NOT 伪造本地等价实现或
把未知字段发送给 strict session schema。

#### Acceptance Criteria

- Given 用户传入上述参数, when app-server 建会话, then create/resume 不包含
  `settings` 或 `maxTurns` 未知字段。
