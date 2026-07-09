# streaming-attachments-and-comfort · design

## Context

0.5.x 把 app-server 流式路径设为默认后,经典 `--prompt` 路径独有的两个能力
出现回归:`@文件` 提及(经典路径经 `extract_file_mentions` →
`build_prompt_command_with_attachments` 翻译成 `--attach`)在流式 send 里
只发 content;项目 `.mcp.json` 在流式会话中完全失效。2026-07-07 对真内核
0.15.0 的 bundle 提取 + 实弹 spike 已把全部形状钉死:

- **附件**(bundle schema `Pwt`,`session/send` 的 `attachments[]`,按
  `kind` 判别的严格 union):
  - `{kind:"image", filename, mimeType, sizeBytes?, dataBase64?, localPath?}`
  - `{kind:"audio", filename, mimeType, dataBase64?, localPath?}`
  - `{kind:"file", filename, mimeType, sizeBytes(必填 int≥0), dataBase64?,
    textContent?, localPath?}`
  实弹:`kind:"file"` 只带 `localPath`(不带 dataBase64)发送,问"附件里的
  暗语是什么"→ 模型逐 token 流回哨兵串 `PLUM-KANGAROO-4711`。
  **localPath 单独可用,无需 base64 回退。**
- **MCP**(bundle schema `$xe`,`session/create`(`Oxe`)与
  `session/resume`(`zxe`)的 `mcpServers[]`,均 optional):
  - stdio:`{name, command, args:[string], env:[{name,value}], timeoutMs?}`
    strict(**没有 type 字段**)
  - 远程:`{name, type:"http"|"sse", url, headers:[{name,value}], timeoutMs?}`
    strict
  实弹三连:① 内核 bundle 中 `.mcp.json` 只出现在 plugin 加载路径——内核
  自身从不读项目 `.mcp.json`;② 在含 `.mcp.json` 的目录裸 create 后
  `mcp/list {workspace}` 返回 `{"statuses":{}}`(bug 证实);③ create 带
  `mcpServers` 后同会话一轮实问,模型报出 `mcp__zq-spike__zq_magic_number`
  (修法证实;`mcp/list` 带同数组返回
  `{status:"connected", transport:"stdio", toolCount:1}`)。
- **resume messages**(结果 schema `Qp.messages: [hwt]`):
  `{info:{role:"user"|"assistant", messageId, time,…},
  parts:[{type:"text", text,…}|{type:"reasoning",…}|{type:"file",
  filename,…}|{type:"step-start"|"step-finish",…}]}`。实弹:关旧会话再
  resume,取回 user(text+file parts)与 assistant(text+step-* parts)。
- **session/close**(params `rve`):`{sessionId}` strict——空 params 实弹
  ZodError 点名 sessionId;`{sessionId}` 返回 `{}`(result `Sxe`)。
- **checkpoint.created**:`session/event` 通知的 `params.type`,payload
  为宽松 record(实测含 checkpointId、messageId、scope、snapshotRef、
  diffRef、`fileCount`),每次被门禁工具落盘一条;payload **没有 kind**,
  现解码器 `decode_app_message` 因取不到 `payload.kind` 直接丢行。

## Goals / Non-Goals

**Goals:**

- 流式路径补齐 @附件 与 MCP 配置两个回归(经典路径不动)
- resume 回放、OSC52 复制、状态栏模型、完成铃、文件小结、调试日志、
  session/close 收尾——全部小步、可单测、失败不伤会话
- /update 脚本对 feed 文件名做 basename 加固

**Non-Goals:**

- 不做 MCP 运行态管理(状态浮层、重连)——只把配置递给内核
- 不做 OSC52 的 tmux passthrough 包裹(文档引导 `set-clipboard on`)
- 不改经典 `--prompt` 路径的附件行为;不做附件的 dataBase64 回退
  (localPath 已实测可用;真需要时再加)
- 调试日志不追求结构化(JSON lines)——人读的行日志够用

## Decisions

1. **附件构造是 lib.rs 纯函数** `build_send_attachments(mentions, cwd)`:
   扩展名→kind/mimeType 映射表(png/jpg/jpeg/gif/webp→image,其余→file,
   mimeType 未知回退 text/plain),`filename`=basename,`localPath`=绝对
   路径,file 类读 `fs::metadata` 拿 `sizeBytes`(读不到→跳过该附件,
   宁缺毋 ZodError)。挂接点两处:握手完成后的首发 send 与快路径 send,
   都从 prompt 过 `extract_file_mentions`(与经典路径同一套越界防护)。
   备选:直接在 `app_send_params` 里做——被否,params 函数保持形状纯粹、
   文件 IO 留在独立构造器里才可单测。
2. **mcpServers 构造复用 McpConfig**:`mcp_servers_param(&[McpConfig])`
   合并项目级+用户级(项目优先,同名去重),`disabled` 跳过;
   `url.is_some()` → 远程形状(type 取 transport_label,非 http/sse 视为
   http),否则 stdio 形状(command 为空跳过)。env/headers BTreeMap →
   `[{name,value}]` 数组。挂在 `session/create` 与 `session/resume` 的
   params 构造上(两者 schema 同字段)。空数组时**不带 key**(行为与
   现状全等,避免向老内核发未知键)。
3. **resume 回放在 lib.rs 解析、main.rs 渲染**:
   `parse_resume_messages(result, limit, cap)` 返回 `Vec<(role, preview)>`
   ——只取 role∈{user,assistant} 且拼接 `parts[].type=="text"` 的文本,
   跳过空串,~400 字符截断加 "…"。渲染:保留既有
   "resumed sess_…(N messages)" 系统行为小节头,后接每条
   `LogKind::System` dim 行(`› `/`• ` 前缀区分 user/assistant)。
   备选:整条走 markdown_lines 全量渲染——被否,回放是"上下文提醒"
   不是"重播",紧凑 dim 才不淹没新对话。
4. **OSC52 直写 stdout**:`osc52_copy_sequence(text, cap)` 纯函数产出
   `ESC ] 5 2 ; c ; <base64> BEL` 字符串(base64 后 ~100KB 截断——按
   **源文本边界**截断再编码,保证 base64 合法);main.rs 在 render 循环外
   用 `io::stdout().write_all` + flush 直发(TerminalGuard 拥有的就是
   stdout,crossterm 排空后写裸序列安全)。base64 编码手撸(60 行内,
   免新依赖)。触发:leader 表加 `y`(`LeaderAction::CopyLastReply`)与
   `/copy` 本地命令,取最后一条 `LogKind::Assistant` 文本。
5. **完成铃与文件小结都挂在收尾点**:`finalize_app_turn`(流式)与
   `finalize_job`(经典 assistant 任务)统一判 `elapsed > 30s` →
   `\x07` 直写 stdout;`UiConfig` 加 `notify: Option<bool>`(默认 on,
   `notify = off` 关),沿 `mouse` 键模式。文件小结只在流式:
   `AppServerTurn` 加 `checkpoints`/`files_changed` 计数,
   `decode_app_message` 对无 `payload.kind` 的 session/event 以
   `params.type` 直通为 `AppServerEvent{kind:<type>}`(payload 的
   `fileCount` 进 `file_count` 字段),`apply` 认 `checkpoint.created`;
   收尾 >0 时落 dim 系统行。取消的回合不响铃(cancel_requested)。
6. **调试日志启动判定一次**:`UiState` 持 `debug_log: Option<File>`
   (启动读 `ZCODE_TUI_LOG`,open append 失败即 None,零开销路径是一个
   `is_none()` 分支)。**红线:出站只记方法名**(`-> session/resume`),
   入站记摘要(方法/kind/reason/id,截断);格式化函数
   `log_line_for_inbound` 为 lib.rs 纯函数并单测"绝不包含 params 序列化"
   ——runtimeModel/apiKey 在 params 里,方法名白名单式摘要从结构上杜绝
   泄漏。会话级转换(握手阶段、turn finalize、downgrade)另有一行。
7. **session/close 尽力而为**:`app_close_params(sessionId)` +
   fire-and-forget send(不进 control_requests——响应无人关心,错误
   静默;`Other`/未匹配 Response 本就被 idle pump 吞掉)。触发点:
   /new 丢弃活会话时、/exit(Quit effect)带活会话时。连接已死则跳过。
8. **/update 加固一行**:`DEB=$(basename "$DEB")` 紧跟解析之后——feed
   若给 `../../etc/cron.d/x.deb` 这类 url,拼进 `curl -o "$TMP/$DEB"`
   会路径穿越;basename 后语义不变(正常 feed 的 url 本就是裸文件名)。

## Risks / Trade-offs

- [附件 localPath 语义依赖内核读本机路径,未来远程 kernel 失效]
  → schema 本就带 dataBase64 备胎;实测当前内核 localPath 可用,真需要
  时按 cap 读文件补 base64,接口(纯函数)已预留形状。
- [mcpServers 每次 create/resume 都送一遍,配置漂移]
  → 每次握手时重读配置文件,天然拿到最新;失败(解析错)按现有
  load_mcp_config 的宽松语义回空,不阻塞握手。
- [OSC52 在不支持的终端静默无效] → 状态栏仍显示 "copied (OSC52)",
  README 注明依赖终端支持与 tmux `set-clipboard on`;不做能力探测
  (探测本身不可靠)。
- [BEL 在静音终端无感] → 本就是尽力而为的提醒;notify=off 留给嫌吵的人。
- [checkpoint 计数把非文件 checkpoint 算进去] → 实测 checkpoint.created
  只由落盘工具触发且带 fileCount;fileCount 缺失按 0 计,行只在 >0 时出。
- [调试日志写盘失败拖慢主循环] → append 单行 + 忽略错误;文件打开失败
  启动即降 None。
- [session/close 后内核还回推事件] → close 只在丢弃会话/退出时发,
  之后要么 app_session=None 要么进程退出,残响无处可污染。

## Migration Plan

纯增量,无迁移。`ZCODE_TUI_LOG` 未设、`notify` 未配、无 `.mcp.json` 时
行为与 0.5.1 全等(mcpServers 空数组不带 key;附件仅在 prompt 有 @提及
时出现)。回滚 = 回退本变更提交。

## Open Questions

- ~~localPath 单独是否可用~~ → 实弹证实可用(见 Context)。
- ~~mcpServers 是否真被会话采用~~ → 实弹证实(模型报出 mcp__ 工具名)。
- ~~session/close 形状~~ → `{sessionId}` strict,实弹 ZodError 钉死。
