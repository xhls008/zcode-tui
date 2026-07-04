# db-live-progress-and-auth-screen

## Why

2026-07-04 流式 spike 实测(设计文档 §5.1):headless `--prompt` 的 stdout
无论纯文本还是 `--json` 都是结尾整块返回,用户在 13-20 秒的运行里只能对着
spinner 干等——这是 SSH 日用回路里最大的体验缺口。同一 spike 发现内核在
turn 进行中**实时写入** `~/.zcode/cli/db/db.sqlite`(part/tool_usage 等表,
首个新行 1.6s 落库,只读轮询无锁冲突),准流式不需要 app-server 协议即可
实现。另外实测暴露认证检测的事实错误:内核硬性要求 `~/.zcode/cli/config.json`,
仅设 env API key 不够,而 TUI 的 /auth 现在会误报已认证;未登录时也缺少一个
像 Codex 那样引导登录的入口屏。

对照设计文档 §9:准流式与工具展示对应"批次 1 放大官方能力"的精神,
db 消费模块同时是批次 1 会话选择器、批次 2 持久历史的共同地基;
不触碰"明确不做"清单(无协议层再造,无桌面 GUI 功能)。

## What Changes

- 新增只读 db.sqlite 消费模块(lib.rs 纯逻辑):schema_migration 校验、
  按 cwd 解析当前会话、拉取 turn 进行中的 part/tool_usage 增量;
  任何异常(缺文件、busy、schema 不识别)→ 整组功能降级隐藏,回到现状行为
- prompt 任务运行期间 ~400ms 轮询 db:工具调用实时渲染为 chip
  (运行中 spinner → 完成 ✓/失败 ✗),reasoning 以 dim 单行显示在工作行,
  **仅运行时显示**,turn 结束丢弃不进 transcript
- prompt 通道改用 `--json`:结尾整块总结对象为权威结果(response 走
  markdown 渲染;sessionId/usage/contextUsed 入状态),状态栏显示
  上下文水位(contextUsed/contextWindow)
- /auth 检测修正:增加 config.json 存在性检查,env key 单独存在时
  显示"部分配置,仍需模型配置"而非已认证
- 新增未登录启动屏:ZCODE 块字标清华紫带阴影,底部鸟巢/长城/天坛轮廓线,
  `›` 引导三条登录路径(/login、`zcode login bigmodel-coding-plan-api-key`、
  `zai-coding-plan-api-key`)
- /login 在无桌面环境(无 DISPLAY/WAYLAND_DISPLAY)自动附 `--no-browser`
- 主题新增 brand 紫 token(logo 专属;不改变 GLM 蓝单一强调色纪律;
  NO_COLOR/--no-color 全部退化)

子进程说明(rules 要求):db 轮询是进程内只读查询,不新增子进程;
prompt 子进程沿用现有进程组 spawn/killpg 取消路径,不改动。

## Capabilities

### New Capabilities

- `db-consumer`: 只读消费内核 db.sqlite 的基础能力——schema 校验与降级
  纪律、当前会话解析(resume/continue/首条快照法)、part/tool_usage 增量
  查询;为准流式、以及后续会话列表/持久历史/todo 视图提供共同地基
- `live-progress`: prompt 运行期间的准流式体验——工具 chip 实时状态、
  reasoning 仅运行时工作行、轮询节奏与失败跳拍
- `prompt-json-result`: prompt 通道的 `--json` 整块权威结果解析——
  response/sessionId/usage/contextUsed 提取、状态栏上下文水位、
  解析失败时按纯文本降级
- `auth-experience`: 认证检测与未登录引导——config.json 检查、
  未登录字符画屏(清华紫/阴影/地标轮廓/登录路径)、/login 无桌面
  自动 --no-browser

### Modified Capabilities

(无——openspec/specs/ 目前为空,本变更全部为新能力)

## Impact

- `src/lib.rs`:新增 db 消费与 --json 总结解析的纯逻辑(全部可单测);
  认证检测函数签名扩展(config.json 路径参数)
- `src/main.rs`:任务泵接入轮询、工具 chip/工作行渲染、未登录屏、
  状态栏水位、Theme 加 brand token
- `Cargo.toml`:新增 rusqlite(bundled feature,已确认与 musl 静态
  构建兼容,release 工作流不受影响)
- `tests/core.rs`:新增 db 消费/解析/降级路径单测
- 文档:README 功能与命令表、设计文档 §2.1/§3/§8 对应小节随实现同步
- 风险:db schema 属内核私有——以 schema_migration 白名单 + 降级纪律
  兜底;首条 prompt 会话行出现时机需实现前迷你 spike 钉死
