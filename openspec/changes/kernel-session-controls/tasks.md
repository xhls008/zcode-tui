# kernel-session-controls · tasks

## 1. 协议层:服务器请求解码与交互解析(lib.rs)

- [x] 1.1 `AppServerMessage` 加 `ServerRequest {id: serde_json::Value, method: String, params: Value}` 变体;`decode_app_message` 把同时含 method+id 的行分派到该变体(id 兼容字符串/数字),数字 id Response 行为不变——单测:字符串 id 识别、数字 id 不受影响、应答编码 id 原样回传
- [x] 1.2 `InteractionRequest::parse(params) -> Option<InteractionRequest>`:提取 request_id / prompt / questions(header, question, options{label,value,description})/ tool_name;缺关键字段返回 None——单测:实测抓取的完整 payload 解析、缺字段容错
- [x] 1.3 应答编码 `encode_interaction_reply(envelope_id, request_id, answers) -> String`(`{"id":…,"result":{"requestId":…,"answers":{…}}}`)——单测:字符串信封 id 原样、answers 映射
- [x] 1.4 spike 钉死 approve 语义与拒绝表达:应答 approve 后观察内核是否期待客户端续跑(mode 是否变化/是否需要 setMode+重发);尝试拒绝候选(非法值/空 answers)记录内核行为——产出记录进 design.md Open Questions 的答案,并按结果微调 2.3/2.4

## 2. 权限确认浮层(main.rs)

- [x] 2.1 UiState 加 `interaction: Option<PendingInteraction>`(request 数据 + 最新信封 id + 选中项)与按 requestId 的已应答集合;`pump_app_turn` 分派 ServerRequest:新 requestId 弹浮层,重发只更新信封 id,已应答静默丢弃——单测(纯逻辑部分):去重状态机
- [x] 2.2 浮层渲染(复用 session-picker 覆盖层模式):prompt/question 文案 + options 列表(label + description),↑↓ 选择、Enter 应答、Esc 拒绝;浮层打开时流式渲染继续
- [x] 2.3 Enter 应答:发 encode_interaction_reply(最新信封 id),关浮层,记入已应答;按 1.4 结论补 approve 后续动作(如需要:setMode + 续跑)
- [ ] 2.4 Esc 拒绝:按 1.4 结论应答拒绝值;无协议级拒绝则关浮层 + session/stop 走既有取消/drain 路径——pty 冒烟:plan 模式写文件 prompt → 浮层出现(pyte screen_seen)→ Esc 后回合不挂起

## 3. 会话控制命令(main.rs + lib.rs 参数构造)

- [x] 3.1 lib.rs:`app_set_mode_params / app_set_model_params / app_set_thought_params / app_compact_params / app_steer_params` 纯函数 + 从 state 推送提取 `model.available[].ref` 与 `thoughtLevel.available` 的解析函数——单测:参数形状与提取
- [x] 3.2 UiState 缓存最近一次 state 推送的控制面(mode/model/thoughtLevel 当前值与候选);`mode_changed` 推送更新缓存并回显状态栏
- [x] 3.3 `/model`:候选列表浮层(label + providerLabel),Enter 发 setModel;无 app-server 会话时提示;失败 push_error 不动会话
- [x] 3.4 `/think`:在 available 档位间切换发 setThoughtLevel;回显以推送为准
- [x] 3.5 `/compact`:发 compact,状态栏 compacting,完成后水位刷新;≥80% 水位提示文案加 /compact 选项
- [x] 3.6 /mode 与 Shift+Tab:app-server 会话活跃时改发 setMode(即刻生效),否则保持既有 CLI 参数行为;slash 补全与 palette 收录新命令
- [ ] 3.7 pty 冒烟:app-server 下 Shift+Tab 切 plan → 状态栏回显(screen_seen);/compact 后水位变化

## 4. Steer 中途转向(main.rs)

- [x] 4.1 handle_key Enter 分支:`app_turn.is_some()` 时非空输入走 session/steer(User 条目落 transcript 标注转向);app_connect/app_draining 期间保持排队——单测:路由条件
- [x] 4.2 steer 错误响应(按请求 id 关联)→ 提示 + 该输入退回排队,回合不中断
- [ ] 4.3 pty 冒烟:流式回合中输入第二条指令 → 不出现 "queued",转向输入落 transcript

## 5. 收尾

- [x] 5.1 README(命令表 + 环境变量段)与 CHANGELOG 更新;/help 文案
- [ ] 5.2 全量门禁:cargo fmt --check / clippy -D warnings / cargo test / pty 冒烟 30+ 项全绿;./install.sh 部署
