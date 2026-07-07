# session-rewind · tasks

## 1. 协议层:检查点解析与 rewind 家族编解码(lib.rs)

- [ ] 1.1 `CheckpointEvent::parse(params) -> Option<CheckpointEvent>`:识别 `params.type=="checkpoint.created"`,提取 checkpointId(必选)与 messageId / targetMessageId / toolMessageId / scope / snapshotRef / diffRef / fileCount(全 Option)——验证:lib 单测(spike 实抓 payload 全形解析、缺 checkpointId 返回 None、缺可选键不失败)
- [ ] 1.2 参数构造纯函数 `app_rewind_params(session_id, target, scope)` / `app_preview_rewind_params(session_id, target)`,target 为 `{kind:"checkpoint",checkpointId}` / `{kind:"latestCheckpoint"}` / `{kind:"turn",turnIndex}` 三型——验证:lib 单测(JSON 形状与 spike 已验参数逐字段一致)
- [ ] 1.3 结果解析:`RewindPreview::parse`(canApply/safeFiles/unsafeFiles/ignoredFiles,unsafeFiles 含 reason/expectedHash/currentHash,currentHash 兼容 `"missing"`)与 `classify_rewind_outcome(triggered_payload, response_text)`(strategy=="unavailable" → 失败 + reason;其余成功)——验证:lib 单测(spike 实抓的 safe/unsafe/假成功三组 payload)
- [ ] 1.4 spike 钉死 scope:"both" 行为与 Edit 工具是否产生 checkpoint.created(脚本入 scratchpad,结论回填 design.md Open Questions;若 both 异常则 UI 隐藏该项)——验证:协议 spike 日志留存

## 2. 检查点累积与 /rewind 浮层(main.rs)

- [ ] 2.1 UiState 加 `checkpoints: Vec<CheckpointEvent>`(绑定当前 sessionId,切会话/新会话清空);pump_app_turn 消费 checkpoint.created 入列——验证:lib 单测(累积/清空状态机纯逻辑部分)
- [ ] 2.2 `/rewind` 命令:app-server 会话空闲时打开目标浮层(latestCheckpoint + 检查点新→旧,显示短 id 与 fileCount);无检查点提示;回合进行中拒绝并提示;非 app-server 路径提示"需要 app-server 会话";slash 补全与 /help 收录——验证:pty 冒烟(screen_seen 浮层标题与候选行;--prompt 路径 screen_seen 提示文案)
- [ ] 2.3 浮层键路由:↑↓ 选择、Enter 进预览、Esc 逐级回退(预览→列表→关闭),复用 session-picker 覆盖层模式——验证:pty 冒烟(screen_seen 各态切换)

## 3. 预览、scope 与应用(main.rs)

- [ ] 3.1 Enter 选中目标 → 发 previewFileRewind,渲染预览页:safeFiles(action/path/toolNames)、unsafeFiles(path/reason/hash 摘要)、canApply;预览错误响应 push_error 并退回列表——验证:pty 冒烟(screen_seen 预览文件行);lib 单测(预览渲染文案纯函数)
- [ ] 3.2 scope 选择(conversation/workspace/both,默认 workspace)+ 确认项:canApply:true 默认"应用",canApply:false 默认"取消"且需显式选中"强制应用"(警示强制覆盖文案)——验证:pty 冒烟(tamper 后 screen_seen 警示文案与默认焦点)
- [ ] 3.3 发 session/rewind,按 classify_rewind_outcome 判定:成功落 response 文案、假成功按失败样式呈现且候选列表不变;成功后按 result.snapshot 刷新(workspace:追加回执行;conversation/both:以 snapshot.messages 重建视图,过滤 model-only synthetic 消息并标注"对话已回滚",候选检查点按目标裁剪)——验证:pty 冒烟(真内核 workspace 回滚后 screen_seen 回执文案 + 磁盘文件断言;假成功用注入 fake 内核脚本);lib 单测(判定与 snapshot 过滤)
- [ ] 3.4 `state.updated reason=="session_rewound"` 的 patch 并入既有控制面缓存路径;任何 rewind 家族错误响应遵循控制命令纪律(push_error、不 kill 会话、后续 prompt 可用)——验证:lib 单测(reason 分派);pty 冒烟(错误后再发 prompt 正常)

## 4. 收尾

- [ ] 4.1 README 命令表 + CHANGELOG + /help 文案收录 /rewind——验证:文档 diff 审阅 + pty 冒烟 /help screen_seen
- [ ] 4.2 全量门禁:cargo fmt --check / clippy --all-targets -D warnings / cargo test / tests/pty_smoke.py 全绿——验证:门禁命令输出全绿
