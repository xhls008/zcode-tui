# streaming-attachments-and-comfort · tasks

## 1. 协议层纯函数(lib.rs + 单测)

- [x] 1.1 附件构造器 `build_send_attachments(mentions, cwd)`:扩展名→kind/mimeType 表(png/jpg/jpeg/gif/webp→image,其余 file+text/plain 回退),filename=basename、localPath=绝对路径、file 类 sizeBytes 必填(metadata 失败跳过该项);`app_send_params_with_attachments` 形状——单测:file/image 形状、未知扩展名回退、空 mentions 不带 key
- [x] 1.2 mcpServers 构造器 `mcp_servers_param(project, user)`:McpConfig 合并(项目优先同名去重)、disabled 跳过、stdio(无 type 字段)/http/sse 两形状、env/headers → [{name,value}];create/resume params 挂接——单测:两形状、去重、disabled、空回 None
- [x] 1.3 resume 回放解析 `parse_resume_messages(result, limit, cap)`:role 过滤、text parts 拼接、空串跳过、末尾 ≤limit、~cap 字符截断加 "…"——单测:实测形状 payload、截断、非文本跳过
- [x] 1.4 OSC52 序列 `osc52_copy_sequence(text, cap)`:手撸 base64、源文本边界截断保证序列合法——单测:已知向量编码、超限截断、空文本 None
- [x] 1.5 解码扩展:`AppServerEvent` 加 `file_count`;`decode_app_message` 对无 payload.kind 的 session/event 以 params.type 直通;`AppServerTurn` 计 `checkpoints/files_changed`,apply 认 checkpoint.created——单测:直通解码、计数、type 也缺失时忽略
- [x] 1.6 `app_close_params(sessionId)`;UiConfig 加 `notify: Option<bool>`(沿 mouse 键模式)——单测:params 形状、notify 解析 on/off/坏值
- [x] 1.7 调试日志格式化纯函数(入站摘要 `log_line_for_inbound`、出站方法名行):红线单测——resume/create 出站行不含 params 内容,入站摘要截断

## 2. 流式路径接线(main.rs)

- [x] 2.1 快路径 send 与握手首发 send 都过 extract_file_mentions → 附件构造 → 带 attachments 的 send params
- [x] 2.2 session/create 与 session/resume 握手 params 附 mcpServers(每次握手重读两级配置;空省略)
- [x] 2.3 resume 成功分支:保留 "resumed …" 头,后接 ≤6 条 dim 回放行(user/assistant 前缀区分)
- [x] 2.4 leader 表加 `y`(LeaderAction::CopyLastReply)与 `/copy` 本地命令:取最后 Assistant 条目 → OSC52 直写 stdout + flush,状态栏回显;无内容提示
- [x] 2.5 footer 右侧:controls.model_current + mode(回退 display_mode(config)),接在水位·认证段
- [x] 2.6 完成铃:finalize_app_turn 与 finalize_job(assistant 类)elapsed>30s 且未取消且 notify!=off 时写 BEL
- [x] 2.7 文件小结:finalize_app_turn 读 turn.files_changed>0 → dim 系统行 `N file(s) changed · /diff to review`
- [x] 2.8 调试日志:UiState 持 Option<File>(启动判定一次),AppServerConn 出入站与握手/收尾/降级转换点埋点
- [x] 2.9 session/close:/new 丢活会话与 Quit effect 带活会话时 fire-and-forget 发送
- [x] 2.10 /update 脚本 `DEB=$(basename "$DEB")` 加固

## 3. 文档与冒烟

- [x] 3.1 help_text、command_catalog(/copy)、README.md(命令/快捷键/环境变量/配置段)、CHANGELOG.md Unreleased
- [x] 3.2 README.en.md 同步近期功能(权限确认、会话控制、/usage /update、默认流式、steer、resume 修复)+ 本批新增;保持既有较简风格
- [x] 3.3 pty_smoke 新场景:流式 @附件 哨兵问答;/copy OSC52 序列出现;footer 模型显示(pyte screen_seen);resume 回放行

## 4. 质量门禁

- [x] 4.1 cargo fmt --check、clippy -D warnings 零告警、cargo test 全绿
- [x] 4.2 cargo build --release + python3 tests/pty_smoke.py 全绿(53 项存量 + 新增)
- [x] 4.3 ./install.sh 部署;按 feat/fix/docs 分组提交
