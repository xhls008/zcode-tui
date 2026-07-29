# browser-use-routing Specification (delta)

## ADDED Requirements

### Requirement: Browser Use 参数不得被 app-server 静默忽略

TUI MUST 显式识别 3.5.3 的 `--browser-use <mode>` 与
`--browser-executable <path>`。只要本 turn 请求 Browser Use，系统 MUST 把
prompt 路由到官方经典 CLI，使参数原样到达 `zcode --prompt`；在没有验证的
app-server browser runtime 协议前 MUST NOT 把参数丢弃或伪造 strict
session 字段。

#### Acceptance Criteria

- Given `--browser-use headless`, when 提交 prompt, then 实际子进程 argv 含
  `--prompt` 与 `--browser-use headless`，且不发送 session/send。
- Given 同时提供 `--browser-executable /path/chrome`, when 提交 prompt,
  then 两个 browser 参数均作为独立 argv 传给官方 CLI。
- Given app-server 功能已启用, when Browser Use turn 被路由经典路径, then
  UI 显示该 turn 不具备 token 流式/steer 等 app-server 控制，而不是静默
  降级。
- Given 没有 browser 参数, when 提交 prompt, then 既有 app-server/经典
  路由选择完全不变。

### Requirement: Browser Use 取值遵循官方 3.5.3 语义

TUI SHALL 只把 `headless` 视为当前已知 mode；`--browser-executable` 没有
配套 headless mode 时 MUST 产生清晰错误。官方 CLI 的进一步校验错误 MUST
原样可见，TUI 不得将失败伪装为普通无浏览器 prompt。

#### Acceptance Criteria

- Given `--browser-use invalid`, when 启动, then 用户看到 mode 不支持的
  错误且不发 app-server 请求。
- Given 仅有 `--browser-executable /path/chrome`, when 启动, then 用户
 看到必须配合 headless mode 的错误。
- Given 官方后续版本接受新 mode, when 本地 parser 尚未知, then 错误应建议
 通过经典官方 CLI 验证，而不是猜测 app-server 字段。

### Requirement: Browser Use 经典任务沿用进程组清理

Browser Use prompt MUST 使用既有 streaming job 的独立进程组；Esc/Ctrl+C
MUST kill 整个进程组，避免浏览器或其子进程在 TUI 退出后残留。

#### Acceptance Criteria

- Given headless browser prompt 正在运行, when 用户按 Esc, then CLI 与其
  子进程组被终止，TUI 回到可输入状态。
