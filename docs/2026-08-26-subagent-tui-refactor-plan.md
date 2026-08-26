# ZCode TUI 工具展示与 Subagent 交互重构计划

日期：2026-08-26

状态：实施中；7 个 Deep Feature 的完整 Feature Map 已初始化，Subagent/后台任务架构拆分的第一个切片已于 2026-08-26 落地

## 背景

当前项目的主要实现集中在 `src/main.rs` 和 `src/lib.rs`。`UiState` 同时承载终端输入、会话、app-server 协议、V4 状态、transcript、工具结果、多个浮层以及后台任务状态。随着官方 Subagent 能力接入，继续在这两个文件中增加状态和渲染逻辑会扩大功能之间的耦合。

当前长输出折叠也不适合继续扩展：`Ctrl+O` 查找最近一条可折叠记录，未进入终端 scrollback 的记录在原位置展开，已进入 scrollback 的记录通过浮层查看。这套机制只能操作“最近一条长输出”，依赖日志数组下标，并且与项目采用的 inline viewport、原生终端滚动和系统复制方向不一致。

本计划先简化工具结果展示，再拆分代码职责，最后接入官方 Subagent 查询、状态展示和控制能力。

## 2026-08-26 实现同步

原计划对“尚未实现”的判断不完整。当前代码已经具备一个早期的只读 `/agents` 流程：

- `src/lib.rs` 已解码 `background_task_started/updated/completed` 的 `taskId`、`toolCallId`、`toolName`、`status`、`pid` 和 `command`。
- `src/main.rs` 已经会缓存后台任务生命周期，并通过 `/agents` 展示只读列表。
- 当前展示的仍是“观测到的后台任务”，还不是完整的 Subagent Inspector；尚未接入 `session/subagents`、V4 `subagents/backgroundWorks`、任务取消和子会话详情。

本次完成了第一个可独立验证的架构拆分：

- 新增 `src/agents.rs`，集中管理 `BackgroundTask` 领域模型、生命周期事件归并、Inspector 打开/关闭和选择状态。
- `UiState` 不再直接持有 `background_tasks` 和 `background_task_picker`，改为组合 `AgentInspectorState`。
- `main.rs` 保留协议事件转发、按键路由和 Ratatui 渲染；任务归并与选择边界改为可独立单测的纯状态逻辑。
- 保持现有 `/agents` 交互、会话重置和后台事件展示行为不变。
- 验证已通过：`cargo fmt --check`、`cargo clippy --all-targets --all-features -- -D warnings`、`cargo build --release`，以及 `cargo test`（12 个二进制单元测试，含 2 个新增 agents 测试；107 个核心集成测试）。

这一步只完成了 PR 2 的第一个垂直切片，不代表 PR 1、PR 2 其余模块或 PR 3/4 已完成。

## 已确认的产品决策

### 1. 移除 `Ctrl+O` 长输出折叠

`Ctrl+O` 不再用于展开或折叠最近的工具、系统、Diff 或错误输出。第一阶段移除绑定，不立即复用该快捷键。

需要删除的状态和行为包括：

- `unfolded: HashSet<usize>`
- `expanded_log`
- `toggle_fold()`
- `handle_expanded_log_key()`
- `render_expanded_log()`
- `fold_preview()` 及对应测试
- 日志插入或删除时修正展开下标的逻辑
- README、帮助面板和命令提示中的折叠说明

移除的是展示层折叠功能，不是协议数据。完整工具结果仍可保留在内核会话、协议事件和调试日志中。

### 2. 默认展示结构化工具摘要

Agent 内部调用的工具默认只在主 transcript 中展示有意义的信息：

| 工具类型 | 默认展示 |
| --- | --- |
| Read | 文件名、读取范围、耗时 |
| Search/Glob | 查询条件、匹配数量、主要文件 |
| Bash | 命令摘要、退出状态、耗时 |
| Edit/Write | 文件名、增删行数 |
| MCP | Server、工具名、成功或失败 |
| Subagent | Agent 名称、任务、阶段状态 |
| 未知工具 | 工具名、输入摘要、状态 |

示例：

```text
• Read README.md · 8ms
• Search "session/subagents" · 12 matches in 3 files
• Bash cargo test · 18.4s · passed
• Edit src/main.rs · +34 -12
```

失败结果必须展示足够的诊断尾部，不能只显示 `failed`：

```text
✗ Bash cargo test · exit 101 · 4.2s
  error[E0308]: mismatched types
  --> src/agents.rs:82
  … 3 more diagnostic lines
```

### 3. 用户主动请求的输出不自动省略

展示策略需要区分来源：

- Agent 内部工具调用：显示结构化摘要。
- 用户主动执行的 `! command`：自然输出到终端 scrollback。
- `/diff`、`/usage`、`/status` 等用户主动请求的报告：完整展示。
- 错误结果：直接展示必要诊断和 stderr 尾部。
- 助手最终回答：完整展示，不折叠。

### 4. Subagent 使用独立交互设计

Subagent 不嵌套展开在父对话的普通工具输出中。父 transcript 只显示简洁的任务状态和最终摘要；详细信息通过 `/agents` 打开的独立 Agent Inspector 查看。

第一阶段 Agent Inspector 是只读查看器。输入框始终发送给父 Agent，界面必须明确显示：

```text
viewing: agent-2 · read-only
input target: parent
```

在官方 app-server 没有公开定向消息 RPC 前，不提供“向所选 Subagent 发消息”的伪功能。

## 官方 ZCode 能力边界

本计划只调用 ZCode app-server 公开能力，不读取或修改用户 API Key，也不自行实现 Subagent 调度。

### 可直接使用

- `session/subagents`：查询运行中和已结束的 Subagent。
- `session/cancelBackgroundTask`：取消带有真实 `taskId` 且可取消的后台任务。
- V4 `subagents`：当前会话的 Subagent 快照和增量状态。
- V4 `backgroundWorks`：Bash/Subagent 后台工作状态。
- `background_task_started/updated/completed`：后台任务实时事件。
- `subagent_spawned/message/stopped`：Subagent 生命周期事件。
- `/expert <task>`、`/expert status/resume/stop`：通过正常 `session/send` 交给内核处理。

### 暂不承诺

- 没有公开的 `session/createSubagent` RPC；创建由官方 Expert 工作流或主 Agent 的官方工具完成。
- 内核内部存在 Subagent message/steer 机制，但没有公开的定向消息 RPC。
- `session/subagents` 返回子会话 ID 和摘要，不直接返回完整子会话 transcript。
- 在确认公开协议能安全读取非活跃 child session 前，不自动 resume 子会话，也不回退为直接读取 SQLite。

## 建议的数据模型

### Transcript

```rust
struct TranscriptEntry {
    id: EntryId,
    source: EntrySource,
    kind: EntryKind,
    content: String,
}

enum EntrySource {
    Parent,
    Agent(AgentId),
    Tool(ToolCallId),
    BackgroundTask(TaskId),
}
```

稳定 ID 用于替代依赖 `Vec` 下标的长期状态。日志插入、删除、rewind 和 resume 不再需要修正展开目标下标。

### Subagent

```rust
struct AgentRecord {
    child_session_id: String,
    agent_id: Option<String>,
    tool_call_id: Option<String>,
    task_id: Option<String>,
    title: String,
    summary: Option<String>,
    status: AgentStatus,
    transcript: TranscriptLoadState,
}
```

### 后台任务

```rust
struct BackgroundWork {
    task_id: String,
    kind: BackgroundKind,
    cancellable: bool,
    output_tail: Option<String>,
    child_session_id: Option<String>,
}
```

`taskId`、`childSessionId`、`agentId` 和 `toolCallId` 必须分别保存。只有官方提供的真实 `taskId` 可以传给 `session/cancelBackgroundTask`。

## Agent Inspector 草案

```text
╭ Agents ──────────────────────────────────────────╮
│ Agents │ Background                              │
├────────┬──────────────────────────────────────────┤
│ parent │ 当前对象的状态、摘要和输出               │
│ agent1 │                                          │
│ agent2 │ transcript / tools / output              │
│ bash-1 │                                          │
╰────────┴──────────────────────────────────────────╯
```

建议操作：

- `/agents`：打开 Inspector。
- `↑/↓`：选择 Agent 或后台任务。
- `Tab`：切换 Agents/Background 或列表/详情。
- `Enter`：进入所选对象详情。
- `PageUp/PageDown`：滚动详情。
- `r`：通过官方接口刷新。
- `x`：取消 `cancellable=true` 且有真实 `taskId` 的任务。
- `Esc`：返回上一级或关闭。

父对话保持简洁：

```text
› 分析项目中的并发问题

● code-reviewer  检查状态管理
✓ tester         运行测试 · 18.4s

• 已汇总 2 个 Subagent 的结果
```

## 状态来源和归并顺序

Subagent 状态按以下来源归并：

1. `session/subagents`：持久化、权威，用于 create/resume 后初始化及手动刷新。
2. V4 `subagents/backgroundWorks`：当前会话的实时快照和增量。
3. `background_task_*`、`subagent_*`：低延迟更新状态、输出和完成结果。

不高频轮询 `session/subagents`。建议在以下时机查询：

- session create/resume 完成后。
- 打开 `/agents` 时。
- 收到相关终态事件后延迟刷新一次。

归并规则需要处理事件乱序，使用 revision 和终态优先策略，避免旧的 `running` 覆盖新的 `success/failed/cancelled`。

## 建议模块边界

第一轮拆分控制在少量职责明确的模块，避免一次拆成大量小文件：

实际执行采用增量迁移：先以单文件 `src/agents.rs` 建立领域边界；当官方 Subagent 查询、V4 归并和 Inspector 渲染接入后，再按下列目标树拆成子模块。这样避免在没有实际职责前预先创建空目录。

```text
src/
├── main.rs
├── app/
│   ├── mod.rs
│   ├── state.rs
│   ├── input.rs
│   └── update.rs
├── protocol/
│   ├── mod.rs
│   ├── legacy.rs
│   ├── v4.rs
│   └── types.rs
├── transcript/
│   ├── mod.rs
│   ├── model.rs
│   └── presentation.rs
├── agents/
│   ├── mod.rs
│   ├── model.rs
│   └── reducer.rs
└── ui/
    ├── mod.rs
    ├── conversation.rs
    ├── composer.rs
    ├── agents.rs
    └── theme.rs
```

职责：

- `main.rs`：启动、终端生命周期、顶层事件循环。
- `app/state.rs`：组合顶层状态。
- `app/input.rs`：按键和命令转为动作。
- `app/update.rs`：动作、协议消息和状态转换。
- `protocol/`：官方 app-server/V4 编解码和协议类型。
- `transcript/`：稳定记录、阶段追加和展示投影。
- `agents/`：Subagent/后台任务状态、查询和事件归并。
- `ui/`：纯渲染，不发送协议请求。

## Feature Workflow Map

`feature-workflow/queue.json` 是 Feature 状态和依赖的唯一真实源；下表是便于阅读计划的同步视图。所有节点因 `parallel_split` 路由为 Deep，已创建具体 spec、task、checklist 和通过的 review gate。

| Feature | 用户价值 | 直接依赖 | 计划阶段 |
| --- | --- | --- | --- |
| `feat-tool-output-clarity` | 无需手动折叠也能获得简洁工具输出，同时保留错误诊断和用户主动请求的完整内容 | 无 | PR 1 |
| `feat-tui-module-boundaries` | 保持现有交互不变，同时建立可测试、可恢复的 app/protocol/transcript/agents/ui 边界 | `feat-tool-output-clarity` | PR 2 |
| `feat-subagent-state-sync` | fresh/resume/picker 和实时事件都能得到一致、不退化的 Subagent 与后台任务状态 | `feat-tui-module-boundaries` | PR 3 |
| `feat-agent-inspector` | 通过独立、只读且不改变输入目标的 Inspector 查看 Agent 和后台工作 | `feat-subagent-state-sync` | PR 4A |
| `feat-background-task-cancel` | 仅使用官方声明可取消的真实 `taskId` 安全取消单个后台任务 | `feat-agent-inspector` | PR 4B |
| `feat-child-transcript-capability` | 用可复现的公开协议证据决定子会话 transcript 是否能安全展示 | `feat-subagent-state-sync` | 后续能力门禁 |
| `feat-subagent-tui-plan` | 对整个计划做跨 Feature 集成、兼容性、终端交互和发布验收 | `feat-background-task-cancel`, `feat-child-transcript-capability` | 根 Feature / 最终验收 |

依赖 DAG：

```text
feat-tool-output-clarity
  → feat-tui-module-boundaries
    → feat-subagent-state-sync
      ├→ feat-agent-inspector → feat-background-task-cancel ─┐
      └→ feat-child-transcript-capability ─────────┘
                                                     ↓
                                           feat-subagent-tui-plan
```

执行波次按依赖、优先级与当前 `max_concurrent=1` 串行排序。所有开发、分支、worktree、合并和标签操作均保留在本地；`auto_push=false` 且 `push_tags=false`，默认不推送代码或标签到远程。

1. Wave 1：`feat-tool-output-clarity`。
2. Wave 2：`feat-tui-module-boundaries`。
3. Wave 3：`feat-subagent-state-sync`。
4. Wave 4：`feat-agent-inspector`。
5. Wave 5：`feat-child-transcript-capability`。
6. Wave 6：`feat-background-task-cancel`。
7. Wave 7：`feat-subagent-tui-plan` 做最终集成验收。

子会话 transcript 能力验证允许以“公开协议不安全或不可用”作为合格结论；该结论不阻断基于摘要和输出 tail 的 Inspector，也不允许回退到 SQLite 或伪造的定向消息功能。

## 实施阶段

### PR 1：移除折叠并改造工具展示

- 删除 `Ctrl+O` 折叠状态、按键、浮层和文档。
- 区分 Agent 内部工具、用户主动命令和用户主动报告。
- 为常见工具增加结构化摘要。
- 失败工具显示有限诊断尾部。
- 保持助手正文、用户命令输出和原生 scrollback 行为不变。

验证点：

- 长 Read/Bash 输出不会淹没父对话。
- 失败命令仍能看到足够诊断。
- `! command`、`/diff` 和用户主动报告不被省略。
- resize、滚轮、系统选择和复制行为不回退。
- 帮助和 README 不再出现 `Ctrl+O` 折叠说明。

### PR 2：职责拆分

- [x] 提取第一个 `agents` 领域模块，包含后台任务模型、事件归并和 Inspector 选择状态。
- [ ] 先盘点 `src/main.rs` 与 `src/lib.rs` 的全部职责并建立 source-to-target 迁移表；后续新增逻辑必须进入对应领域模块，不能继续堆回两个主文件。
- [ ] 将 `main.rs` 收敛为启动、终端生命周期和顶层事件循环，将 `lib.rs` 收敛为有意设计的可复用公共 API。
- [ ] 继续提取 protocol、transcript、ui 和 app 状态模块。
- 为 transcript entry 引入稳定 ID。
- 保持现有交互和输出不变。
- 将协议解析和展示投影改为可独立单测的纯函数。

验证点：

- app-server streaming、classic fallback、resume、rewind、model 和 interaction 流程不变。
- 80/120 列终端以及 resize 后渲染快照稳定。
- `cargo test`、`cargo fmt --check`、`cargo clippy` 和 release build 通过。

### PR 3：官方 Subagent 数据接入

- 实现 `session/subagents` 请求和响应解析。
- 解析 V4 `subagents/backgroundWorks`。
- 补齐 `background_task_*` 和 `subagent_*` 字段。
- Bash 后台任务与 Subagent 分离。
- create/resume 后恢复持久化 Subagent 状态。
- 实现 revision 和终态优先归并。

验证点：

- fresh、resume 和 session picker 三条路径都能得到正确列表。
- Bash 与 Subagent 不混淆。
- 乱序事件不会让终态退回 running。
- 旧版内核返回 Method not found 时安全降级。

### PR 4：Agent Inspector 和单任务取消

- `/agents` 使用新的独立 Inspector。
- 展示 Agent 状态、摘要、关联后台工作和输出 tail。
- 仅对真实 `taskId` 且 `cancellable=true` 的任务提供取消。
- 明确显示当前查看对象和输入目标。

验证点：

- Inspector 不捕获终端系统复制快捷键。
- 运行中事件更新不会破坏选择或滚动位置。
- 取消请求只发送官方 task ID。
- 取消失败不会关闭父会话或中断正常 turn。

### 后续验证：子会话 transcript

独立验证以下问题后，再决定是否加入 Agent 对话详情：

- `session/messages` 或 `session/events` 能否读取非活跃 child session。
- 是否必须 resume child session。
- resume 是否会改变父子关系、运行时状态或输入路由。
- 已结束子会话能否继续读取。

如果公开协议不能安全读取，不直接读取内核 SQLite，也不展示伪造的完整对话。

## 非目标

- 不修改官方 ZCode 二进制。
- 不读取、保存或转发用户 API Key。
- 不自行实现 Subagent 调度器。
- 不在没有公开 RPC 时实现定向 Subagent 消息。
- 不在父 transcript 中嵌套所有子 Agent 的完整过程。
- 不为了拆文件进行一次不可审查的大规模重写。

## 完成标准

- 主 transcript 不再依赖用户手动折叠长工具输出。
- 工具展示简洁，同时失败诊断和用户主动请求的内容不丢失。
- app-server 协议、状态归并和 Ratatui 渲染具有清晰模块边界。
- `/agents` 能从官方接口恢复并实时展示 Subagent。
- 用户可以查看和取消官方声明可取消的任务。
- 界面不会把“查看 Subagent”误导为“输入已经切换到 Subagent”。
