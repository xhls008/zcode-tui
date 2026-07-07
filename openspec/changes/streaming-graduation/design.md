# streaming-graduation · design

## Context

kernel-session-controls 之后流式路径功能完整,wrapper 已默认开启
(install.sh `ZCODE_TUI_APP_SERVER="${ZCODE_TUI_APP_SERVER:-1}"`),本变更
补齐最后一段并把默认落到二进制本体。2026-07-07 spike(kernel 0.15.0 /
桌面包 3.2.5)已钉死:

- `session/resume {sessionId}` 实弹成功,返回与 create 同形
  (messages/projection/session/settings/slashCommands/todos/todoGroups)。
- `session/list {}` 空参可用,返回 sessions[]{sessionId,title,titleSource,
  status,mode,workspace{workspaceKey,workspacePath},createdAt,updatedAt}。
- `session/usage {sessionId}` → totalTokens/inputTokens/outputTokens/
  reasoningTokens/cache*/modelRequestCount;`usage/stats {range:"7d"|"30d"}`
  → summary 含 cacheHitRate/totalSessions(zod enum 钉死值域)。
- steer 响应 result 是判别 union(bundle `FKr`):
  `{kind:"queued",pendingInputId,queueLength,turnId}` |
  `{kind:"rejected",activeTurnId?,reason:enum}` —— 当前实现只看信封错误,
  rejected 会静默丢。
- create/resume 的 slashCommands[]{name,description,inputHint,source}。
- 内核自更新可行性已人工走通一遍(feed→deb→sha512(base64)→dpkg),
  本机免密 sudo 可用;README 已有免 root 解包路径作为退路。

## Goals / Non-Goals

**Goals:**
- resume/list 接入流式路径;/sessions 数据源切换(协议优先,db 回退)
- 二进制默认开启(=0/off/false 退出);冒烟经典场景显式置 0
- steer rejected 处理;/update;/usage;todos 工作区;slashCommands 补全

**Non-Goals:**
- rewind/fork(下一变更 session-rewind,schema 已钉:target 判别 union
  turn/message/checkpoint/latestCheckpoint + scope conversation/workspace/both)
- /sessions 的 db 数据源删除(经典路径仍用)
- AppImage 更新通道(只做 deb + 免 root 解包指引)

## Decisions

1. **resume 融进既有握手状态机**:ConnectPhase 增加 Resume(id) 分支——
   config.resume 存在时首请求发 session/resume,响应处理与 Create 完全同构
   (同形结果,提 sessionId → subscribe)。resume 错误响应不downgrade,
   改发 create 重走(一次性回退,提示一条 dim)。
2. **/sessions 数据源**:app_conn 活跃时用 request_blocking("session/list")
   (短超时 3s)拿列表,失败静默回退 db。选择结果统一写 config.resume,
   两条路径共享既有语义。
3. **默认开启的实现**:`app_server_enabled` 改为 "未设置或非关闭值 → true";
   关闭值 = 0/off/false/no(大小写不敏感)。s1/s2/s7 等假定经典路径的冒烟
   场景显式传 `ZCODE_TUI_APP_SERVER=0`(s11 降级场景仍用假 bin 强制失败)。
4. **steer result 解析进 lib**:`parse_steer_result(&Value) -> SteerOutcome
   {Queued, Rejected(reason)}` 纯函数;pump 的 Response 成功分支对
   ControlReq::Steer 调它,Rejected → 提示 + requeue(复用 on_control_error
   的退队逻辑)。
5. **/update 用既有 job 基建**:下载走 spawn_streaming_command(curl,进度
   行进 transcript 可折叠),完成回调里校验 sha512(rust sha2? 不加依赖——
   用 `openssl dgst` 子进程,机器已有)再 `sudo -n dpkg -i`(again 子进程,
   输出落 transcript)。全程可 Esc(既有取消路径)。版本比较复用
   is_newer_version;feed 解析复用 parse_update_feed(+扩展抓 sha512 字段)。
6. **todos 渲染在工作区**(live 区,不进 transcript):解析进
   SessionControls 同级的缓存,poll/pump 更新;仅非空显示,避免占屏。
7. **slashCommands 合并**:create/resume 结果解析出列表存 UiState,
   slash_suggestions 增加动态命令参数(lib 函数加参,不动静态目录);
   本地命令优先去重。

## Risks / Trade-offs

- [默认反转惊吓存量用户] → wrapper 早已默认开,二进制直用者是少数;
  CHANGELOG 醒目标注 + `=0` 一键回退;降级纪律保证最坏也就是回到 --prompt。
- [resume 到 running 状态的会话] → session/list 结果带 status,选择器对
  running 会话标注;resume 它的行为实现期实测,异常按"resume 失败回退
  create"路径兜底。
- [/update 下载中断/网络抽风] → curl --retry + sha512 硬校验,失败即中止
  删文件;绝不 dpkg 未校验文件。
- [usage/stats 在旧内核不存在] → 错误响应按命令关联提示,不降级(既有
  control_requests 纪律)。

## Open Questions

- resume 返回的 messages 是否值得回放进 transcript(体量可能大;v1 先不
  回放,只提示"已续接 N 条历史"),实现期看 messages 形状决定。
