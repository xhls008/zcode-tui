# app-server-streaming

## Why

2026-07-05 逆向确认(设计文档 §5 与记忆 [[zcode-app-server-protocol]]):headless
`--prompt` 通道把内核内部的 token 流(Anthropic 式 `text_delta`)攒成整块才
返回,所以单轮纯问答无法流式——这是当前最大的体验缺口。内核的 `app-server`
是一条已跑通的 stdio 协议(`session/create` → `subscribe` → `send`,流式经
`state.updated` 修订补丁推送),是**唯一**能让单轮问答也流式的路径,同时
顺带解锁 /model 切换、compact、rewind、权限审批中继等一批 `--prompt` 做不到的
能力。

对照设计文档 §9「明确不做:无加法需求的协议对齐」——本变更是**有明确加法
需求**(真流式)驱动的按需协议消费,不是无差别追平;且严格分阶段、带试验
开关与降级兜底,不动现有 `--prompt` 稳定路径。

## What Changes

- 新增最小 Rust app-server 客户端(lib.rs 纯逻辑 + main.rs 连接管理):
  换行分隔 JSON 信封 `{id, method, params}`(非 JSON-RPC),
  `session/create`→`subscribe`→`send` 生命周期,消费 `state.updated`
  修订补丁重建正在增长的助手正文
- prompt 提交路径在**试验开关** `ZCODE_TUI_APP_SERVER=1` 下改走 app-server
  流式;默认仍走现有 `--prompt`。协议任何环节失败 → **无缝降级**回
  `--prompt`,用户不感知
- 流式渲染:助手正文按 token 增量实时进 transcript(真流式,单轮问答亦然);
  工具调用/reasoning/上下文水位改吃协议权威事件而非 db 轮询
- 取消路径:`session/stop` 替代 killpg(协议内会话);进程组兜底保留
- 连接健壮性:协议版本/schema 不符或 app-server 起不动 → 记一条 dim 提示
  并永久降级本进程

明确不做(留待后续变更,按需再开):`setModel`/`compact`/`rewind`/`fork`/
`steer`/权限审批中继——本变更只打通**流式正文**这一条最小闭环,其余边界
功能待流式地基稳定后各自立项。

子进程/取消说明:app-server 是长驻子进程(独立进程组),prompt 期间不 spawn
新进程;取消发 `session/stop`,连接关闭时 killpg 兜底清理,无残留。

## Capabilities

### New Capabilities

- `app-server-client`: app-server stdio 协议的最小客户端——信封编解码、
  会话生命周期(create/subscribe/send/stop)、`state.updated` 修订补丁
  应用、连接健壮性与降级纪律
- `streaming-prompt`: 试验开关下 prompt 走 app-server 的真流式正文渲染,
  失败无缝降级 `--prompt`

### Modified Capabilities

(无——现有 `prompt-json-result`/`live-progress` 能力在开关关闭时行为不变;
开关开启时由本变更的新能力接管,不修改已归档需求)

## Impact

- `Cargo.toml`:无需新依赖(serde_json 已在;stdio 用 std::process)
- `src/lib.rs`:协议信封编解码、`state.updated` patch 应用、会话状态模型
  (全部纯逻辑,可单测)
- `src/main.rs`:app-server 子进程连接管理、流式事件泵接入主循环、
  降级切换、`session/stop` 取消
- `tests/core.rs`:协议编解码/patch 应用/降级判定单测;
  `tests/pty_smoke.py`:开关开启下的单轮问答流式冒烟(pyte 重建屏幕)
- 文档:README 功能/环境变量、设计文档 §5/§2.1、CHANGELOG(0.4.0)
- 风险:协议未公开、随内核升级可能变——试验开关 + 默认 `--prompt` +
  失败降级把风险完全隔离在可选路径
