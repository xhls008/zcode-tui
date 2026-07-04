# design — db-live-progress-and-auth-screen

## Context

流式 spike(设计文档 §5.1)确立的事实:stdout 整块返回;内核实时写
db.sqlite;`part` 表即完整事件流(text/reasoning/tool 带状态/step-finish
带 tokens);`--json` 是含 usage/contextUsed 的单个总结对象;`/auth` 的
env-key-即-已认证判断与内核实际要求(config.json 硬性)不符。

现有架构约束:lib.rs 纯逻辑必须可单测;main.rs 80ms 主循环轮询 JobEvent;
prompt 子进程在独立进程组,killpg 取消;主题 token 体系 + NO_COLOR 退化;
GLM 蓝单一强调色纪律。

## Goals / Non-Goals

**Goals:**

- prompt 运行期间用户能看到真实进度(工具名+状态、reasoning 片段)
- 一个可复用的 db 只读消费地基(后续会话列表/持久历史/todo 直接受益)
- 认证状态判断与内核事实一致;未登录时有清晰的引导入口
- 任何 db 异常都不影响既有功能(降级 = 完全回到今天的行为)

**Non-Goals:**

- 不做 token 级文本流式(要 app-server 协议,明确不追)
- 不做会话列表/持久历史/todo 视图(下个 change,本次只打地基)
- 不改 shell(`!`)与 /diff 任务的行为(它们本来就是真流式)
- 不做权限审批中继

## Decisions

**D1. db 访问用 rusqlite bundled,只读 URI 连接。**
`file:...?mode=ro` + busy_timeout(100ms);每拍新建连接(简单、无状态,
400ms 节奏下开销可忽略;常驻连接会在内核迁移 schema 时拿着旧句柄)。
备选:常驻连接+重连逻辑——复杂度不值得。sqlite3 CLI 外调被否:引入
子进程与解析脆弱性。

**D2. schema 白名单校验一次,失败即永久降级(本次进程内)。**
启动探测线程里读 `schema_migration` 的 id 集合,与实现时点的已知集合比对:
已知集合是子集 → 启用(允许内核追加新迁移);缺任何已知迁移 → 降级。
降级后所有 db 功能隐藏,UI 与现状完全一致。备选:每拍校验——浪费;
按表探测——迁移可能改列语义,id 白名单更诚实。

**D3. 会话锁定三分法 + 快照法兜底。**
`--resume sess_x`:直接用;`--continue`:spawn 前查
`session WHERE directory=? ORDER BY time_updated DESC LIMIT 1`;
首条 prompt:spawn 前记 `MAX(part.rowid)` 基线,轮询时取
`rowid > 基线 AND session.directory = cwd` 的行。实现前迷你 spike
钉死"全新目录首条 prompt 的 session 行何时出现"(预期 turn 开始即建行;
若更晚,快照法仍然正确,只是前几拍无归属行,可接受)。

**D4. 轮询挂在现有 80ms 主循环上,按拍计数分频(每 5 拍 ≈ 400ms)。**
不起新线程:轮询是纯只读快查(<1ms),放主循环消除并发心智负担;
任何 OperationalError 跳过该拍。备选:独立线程+mpsc——与 JobEvent
泵对称但为 <1ms 的查询引入线程生命周期管理,不值。

**D5. 运行时进度渲染为"工作区"而非 transcript 单元。**
工具 chip 列表 + reasoning dim 行归属 spinner 工作行下方的临时区域,
turn 结束整体消失;权威结果来自 `--json` 总结对象(response → markdown
transcript 单元)。不把轮询到的 part 文本写进 transcript,避免与权威
结果重复/顺序错乱。工具 chip 完成态样式:`⚙ Read notes.txt ✓ 0.3s`,
失败 `✗`(语义红,符合既有红绿语义纪律)。

**D6. --json 解析失败按纯文本降级。**
总结对象解析不出(未来内核改格式)→ 整个 stdout 按现状纯文本/markdown
处理,水位与 sessionId 缺省。保证前向兼容。

**D7. /auth 三态:未配置 / 部分配置(env key 有、config.json 无)/ 已配置。**
部分配置态显式提示"内核还需要 ~/.zcode/cli/config.json,运行
zcode login <plan>-api-key <key> 补齐"。未登录屏在"未配置"与"部分配置"
两态都显示(部分配置时提示语不同)。

**D8. 未登录屏:清华紫 brand token,logo 专属。**
Theme 加 `brand`(亮化清华紫,参考 #82318E 提亮至暗底可读)与
`brand_dim`(阴影用暗紫)。块字主体 brand 色,阴影用 `brand_dim` 的
`░/▒` 偏移行;底部一行鸟巢/长城/天坛轮廓线(复用现有 ZCODE_LOGO 的
地标字符画元素,挪到底部、去清华校门)。强调色纪律不破:紫只出现在
logo 区域,交互元素仍 GLM 蓝;NO_COLOR 全退化。

**D9. 无桌面判定:`DISPLAY` 与 `WAYLAND_DISPLAY` 均未设 → /login 自动附
`--no-browser`。** 纯逻辑放 lib.rs(传入 env 快照,可单测)。

## Risks / Trade-offs

- [db schema 变更] → D2 白名单降级;README/设计文档明示"官方升级后
  db 功能可能自动隐藏,更新 TUI 恢复"
- [首条 prompt 会话行时机未知] → 实现前迷你 spike(tasks 第一项);
  快照法在最坏情况下也只是延迟归属
- [同 cwd 并发两个 zcode 实例] → 按最新 time_updated 归属,可能混入
  他者进度;罕见、只影响显示、不影响正确性,接受
- [--json 改变 prompt 输出契约] → D6 降级路径 + 单测锁双格式
- [轮询在超大 part 表上变慢] → 查询全部走 rowid/session_id 索引界定,
  LIMIT 保护;实测 <1ms,留 10ms 预算告警日志
- [紫色在浅色终端可读性] → 亮化变体按暗底调;浅底终端本就非目标环境,
  NO_COLOR 是兜底

## Open Questions

(实现前唯一待钉:D3 的会话行出现时机 spike——已列为 tasks 第一项,
结果只影响首几拍的归属策略,不影响架构。)
