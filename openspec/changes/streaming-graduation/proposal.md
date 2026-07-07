# streaming-graduation

## Why

app-server 路径功能上已是 `--prompt` 的超集(真流式+权限确认+会话控制+steer),
wrapper 已默认开启(install.sh `:-1`),但还差最后一段:**流式路径的
`/resume`/`--resume`/会话选择器被无声忽略**(session/create 永远开新会话,
与已修的 --mode 同类 bug);二进制本体默认仍关;steer 的被拒响应
(`result.kind:"rejected"`)未处理会静默丢输入。同时内核 3.2.5 手动升级流程
(feed→下载→校验→安装)值得固化为 /update。全部 schema 已由 2026-07-07
spike 钉死(resume/list/usage/stats 实弹验证)。

## What Changes

- **流式路径会话续接**:`/resume [sess_id]`、`--resume`、`/sessions` 选择器
  在 app-server 会话下走 `session/resume {sessionId}`(实弹:返回与 create
  同形,含 messages/todos/todoGroups);`/sessions` 流式下改用
  `session/list {}`(空参可用,返回 title/status/workspace)替代 db 轮询,
  db 路径保留为经典路径的实现。
- **二进制默认开启 app-server**:`app_server_enabled` 语义反转——默认开,
  `ZCODE_TUI_APP_SERVER=0/off/false` 关;降级纪律不变(任一环节失败永久
  回落 --prompt)。冒烟中假定经典路径的场景显式置 0。
- **steer 被拒不丢输入**:steer 成功信封的 result 是 union
  `{kind:"queued"|"rejected", reason}`;rejected(no_active_turn/
  turn_not_steerable/…)时提示原因并把输入退回排队。
- **/update 内核自更新**:读官方 feed(app-update.yml → latest-linux.yml),
  有新版则下载 deb + sha512 校验(feed 内 base64)+ 安装(优先 `sudo -n
  dpkg -i`,不可用则提示免 root 解包命令);更新 Tip 文案加 `/update`。
- **/usage 用量**:`session/usage {sessionId}`(会话级 token 细分)+
  `usage/stats {range:"7d"|"30d"}`(周期汇总,含 cacheHitRate/totalSessions)。
- **TODO 显示**:create/resume 结果与 state 推送中的 todos/todoGroups
  渲染为工作区清单(有内容才显示)。
- **内核 slashCommands 并入补全**:create/resume 结果的
  `slashCommands[]{name,description,inputHint,source}` 合入 `/` 补全
  (本地命令优先,同名去重)。

## Capabilities

### New Capabilities

- `streaming-session-lifecycle`: 流式路径的 resume/list 续接与 /sessions
  数据源切换
- `kernel-self-update`: /update 自更新命令(feed→下载→校验→安装→提示重启)
- `usage-and-todos`: /usage 命令与 TODO 清单显示;内核 slashCommands 并入补全

### Modified Capabilities

- `streaming-prompt`: 默认开启语义反转(opt-in → opt-out),降级纪律不变
- `turn-steering`: steer 被拒(result.kind:"rejected")时按 reason 提示并退回
  排队,不再静默丢

## Impact

- `src/lib.rs`:`app_server_enabled` 反转;`app_resume_params`/
  `app_usage_params`/`usage_stats_params`/list 结果解析/steer result 解析/
  slashCommands 解析/todos 解析(纯函数,单测)
- `src/main.rs`:start_app_prompt 的 resume 分支、/sessions 流式数据源、
  /update 命令(后台任务型下载)、/usage、todo 工作区渲染、steer 响应处理
- `tests/pty_smoke.py`:经典路径场景显式 `ZCODE_TUI_APP_SERVER=0`;新增
  流式 resume 场景;/usage 冒烟
- README/CHANGELOG//help;发版 0.5.0 在本变更落地后进行
