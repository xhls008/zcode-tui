# session-rewind

## Why

app-server 路径已打通流式、权限确认与会话控制,但内核的检查点/回滚能力
(`session/rewind` 家族)在 TUI 完全没有暴露——模型写坏文件或对话跑偏后,
用户只能手工 git 恢复或 /new 丢会话。2026-07-07 协议 spike 已在真内核
(kernel 0.15.0 / desktop 3.2.5)实弹钉死检查点事件、预览与回滚的全部
参数形状、事件时序与磁盘语义(文件确实原地还原,见 design.md),实现风险低。
设计文档 §5 命令表将 /rewind 列为"仍未接(按需评估)",本变更即该评估的落地。

## What Changes

- **检查点捕获**:消费 `session/event` 中 `params.type == "checkpoint.created"`
  的通知(payload 含 checkpointId / messageId / toolMessageId / scope /
  snapshotRef / diffRef / fileCount,实测),按会话累积为可回滚目标列表。
- **/rewind 命令 + 目标选择浮层**:候选 = 本会话捕获的检查点(新→旧)+
  `latestCheckpoint`;↑↓ 选择,Enter 先预览(`session/previewFileRewind`
  结果:canApply / safeFiles / unsafeFiles),再选 scope
  (workspace / conversation / both,默认 workspace)确认应用,
  Esc 逐级退出。
- **安全应用路径**:文件回滚走 `session/applyFileRewind`(尊重
  canApply,拒绝覆盖被会话外修改的文件);对话回滚走
  `session/rewind scope:"conversation"`;both = 文件段成功后再链对话段。
  实测裸 `session/rewind` 会**无视 canApply 强制覆盖**外部修改,因此
  文件 scope 绝不使用它,也不提供强制覆盖路径。
- **结果回执**:显示应用回执的 `response` 一行文案;成功判定基于
  `rewind.triggered` 事件的 `strategy`(applyFileRewind 则以 `applied`
  布尔为权威)。实测内核对"检查点不存在"返回**成功信封 + 失败文案**
  (`rewind.triggered` payload `strategy:"unavailable"`),TUI 按失败呈现。
- 全部仅 app-server 流式路径;`--prompt` 路径行为不变。任何失败遵循既有
  控制命令纪律:报告错误、不 kill 会话、不影响后续 prompt。

## Capabilities

### New Capabilities

- `session-rewind`: 检查点事件(`checkpoint.created`)捕获与解析、/rewind
  目标选择浮层、previewFileRewind 预览呈现、scope 选择、session/rewind
  应用与回执(含"成功信封 + 失败文案"的识别)、不安全文件警示

### Modified Capabilities

(无——`app-server-client` 的信封编解码与 session/event 消费要求不变,
检查点 payload 解析作为新能力内的纯函数落在 session-rewind)

## Impact

- `src/lib.rs`:checkpoint payload 解析、rewind / previewFileRewind 参数
  构造、preview 结果与 rewind 回执解析、"假成功"判定(纯函数,可单测)
- `src/main.rs`:/rewind picker 浮层(复用 session-picker / 权限浮层的
  覆盖层模式)、预览页渲染、scope 选择、应用后回执落 transcript;
  slash 补全与 /help 收录
- `tests/core.rs`:解析/编码/判定单测;`tests/pty_smoke.py`:/rewind 浮层
  冒烟(pyte screen_seen)
- 无新依赖;无子进程新增(复用既有 app-server 连接,取消/清理路径不变)
