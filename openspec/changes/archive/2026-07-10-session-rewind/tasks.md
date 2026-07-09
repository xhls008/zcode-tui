# session-rewind · tasks

## 1. 协议层:检查点解析与 rewind 家族编解码(lib.rs)

- [x] 1.1 检查点事件解析:`decode_app_message` 的 `AppServerEvent` 扩展 `checkpoint_id`(payload.checkpointId,连同既有 file_count 复用 files-changed 小结的解码通道,不另起解析器);缺 checkpointId 只失去目标资格、不破事件流——验证:lib 单测 `checkpoint_event_surfaces_id_for_rewind_targets`(spike 实抓 payload 全形、缺 id 容错)✅
- [x] 1.2 参数构造纯函数 `app_rewind_params(session_id, target, scope)` / `app_file_rewind_params(session_id, target)`(preview 与 apply 同参),`RewindTarget` 枚举 `{kind:"checkpoint",checkpointId}` / `{kind:"latestCheckpoint"}` / `{kind:"turn",turnIndex}` 三型——验证:lib 单测 `rewind_params_match_pinned_shapes` ✅
- [x] 1.3 结果解析:`parse_rewind_preview`(canApply/safeFiles/unsafeFiles/ignoredFiles,currentHash 兼容 `"missing"`)、`parse_apply_file_rewind`(applied/response/嵌套 unsafeFiles,缺 applied 不算成功)、`rewind_failure(strategy, reason, response)`(unavailable → 失败;未见 rewind.triggered 不宣称成功)+ `rewind.triggered` 事件解码出 strategy/reason——验证:lib 单测 3 个(safe/unsafe/拒绝/假成功全组 spike payload)✅
- [x] 1.4 实现期实弹补钉(全部回填 design.md Context):applyFileRewind 应用成功也发 `rewind.triggered`(strategy=active_chain, **reason=file_summary_rewind**)且状态推送 reason 为 **session_file_rewind_applied**;**scope 对 checkpoint 目标不生效**——`session/rewind {target:latestCheckpoint, scope:"conversation"}` 被强制转为 workspace 文件回滚(rewind.triggered 回报 scope:"workspace",删除了外部篡改文件),仅 `{kind:"message"}` 目标尊重 conversation scope(文件未动、messages 收缩到 1)——因此对话段增加 `conversation_target()` 翻译层;scope:"both" 客户端拆为 applyFileRewind + message 目标 conversation 链(Decision 4/8);Edit 是否产生 checkpoint 仍开放(Open Questions)——验证:协议 spike C/D(scratchpad rewind_conv_scope_probe.py / rewind_msg_target_probe.py)+ 一次性 pty 驱动日志 ✅

## 2. 检查点累积与 /rewind 浮层(main.rs)

- [x] 2.1 UiState 加 `checkpoints: Vec<CheckpointEntry>`;pump_app_turn 与 pump_app_idle 经 `capture_rewind_event` 入列(idle 路径同时捕获 rewind.triggered);/new、/resume、降级、握手重建、取消握手、drain 超时共 7 处 `reset_rewind_state` 清空——验证:cargo test 全绿 + 冒烟 s22 debug.log 显示捕获与清空转换 ✅
- [x] 2.2 `/rewind` 命令:app-server 会话空闲时打开目标浮层(latestCheckpoint + 检查点新→旧,短 id + 序号 + fileCount + 前像提示);无检查点提示;回合进行中(app_turn/app_connect/app_draining)拒绝并提示;非 app-server 路径提示;classify_input/command_catalog//help 收录——验证:pty 冒烟 s22(浮层)+ s23(非 app-server 提示)+ 单测 `rewind_is_a_local_command` ✅
- [x] 2.3 浮层键路由:↑↓ 选择、Enter 进预览、预览页 ←/→(或 ↑↓)循环 scope、Esc 逐级回退(预览→列表→关闭);busy 防抖防双发——验证:pty 冒烟 s22 全流程(列表→预览→应用)✅

## 3. 预览、scope 与应用(main.rs)

- [x] 3.1 Enter 选中目标 → previewFileRewind(ControlReq::RewindPreview),预览页渲染 safeFiles(action/path/tools)/unsafeFiles(reason/path)/ignored/canApply;预览错误响应 push_error 退回列表、浮层不卡 busy——验证:pty 冒烟 s22 "rewind preview" screen_seen;单测预览解析 ✅
- [x] 3.2 scope 选择(workspace/conversation/both,默认 workspace);canApply:false 时文件 scope 本地拒绝并警示(不提供强制覆盖,spec 修订版),conversation 仍可选——验证:一次性 pty 驱动(外部 tamper → "files changed outside the session" + "rewind blocked" screen_seen,tampered 文件未被碰)✅
- [x] 3.3 应用:文件段一律 `session/applyFileRewind`(applied 权威;拒绝报 unsafeFiles 原因),对话段 `session/rewind scope:"conversation"` 以 rewind_failure(rewind.triggered strategy)判成败,假成功按失败呈现且候选列表不变;both=文件段成功后链对话段;conversation 成功后标注"对话已回滚"并按目标裁剪候选——验证:pty 冒烟 s22(真内核文件回滚落盘断言 two→one)+ 一次性驱动(conversation scope 实弹 "rewound (conversation)")+ 单测 `rewind_outcome_judged_by_trigger_event_not_envelope` / `apply_file_rewind_refusal_and_success_parse` ✅
- [x] 3.4 状态推送(session_rewound / session_file_rewind_applied)走既有 merge_controls 缓存路径;rewind 家族错误响应遵循控制命令纪律(on_control_error 新变体:报告、复位/关浮层、不 kill 会话);send 失败同样不卡浮层;全程 ZCODE_TUI_LOG 记录状态转换——验证:cargo test 全绿;冒烟 s22 后 /exit 干净收尾;debug.log 转换行留存 ✅

## 4. 收尾

- [x] 4.1 README(特性段 + 命令表)+ CHANGELOG Unreleased + /help 文案收录 /rewind——验证:文档 diff 审阅 ✅
- [x] 4.2 全量门禁:cargo fmt --check / clippy --all-targets --all-features -D warnings / cargo test(92)/ cargo build --release / tests/pty_smoke.py(64+6 全绿)——验证:门禁命令输出 ✅
