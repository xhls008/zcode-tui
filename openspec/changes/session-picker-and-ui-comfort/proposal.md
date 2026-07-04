# session-picker-and-ui-comfort

## Why

0.2.0 落地了 db 只读消费地基后,界面增强清单(2026-07-04 讨论)剩余四项
——会话/任务列表、持久历史、鼠标滚动、消息折叠、主题配置——的原料已经
全部就位:`session` 表有标题与时间、`input_history` 表由内核持久化输入、
折叠与主题是纯 TUI 侧工作。本批把这些"日用舒适度"一次挖完,对应设计
文档 §9 批次 1(会话选择器)与批次 2(持久历史)的条目。

## What Changes

- `/sessions` 会话选择浮层:读 `session` 表列出最近会话(标题/目录/时间,
  当前目录的排前),↑↓ 选择、Enter 设为 `--resume`、Esc 关闭;db 降级时
  命令报"不可用"而不是消失
- 持久输入历史:启动时从 `input_history` 表读入历史(与本进程历史合并),
  `Ctrl+R` 反向搜索浮层(子串匹配,新→旧),Enter 取回输入框
- 鼠标滚轮滚动 transcript(SGR 捕获,`ZCODE_TUI_NO_MOUSE=1` 或配置关闭;
  提示用户按住 Shift 可用终端原生选择)
- 长输出折叠:Tool/System/Diff/Error 单元超过阈值行数默认折叠为
  头部预览 + `… (+N lines · Ctrl+O)`,`Ctrl+O` 展开/收起最近一个可折叠单元
- 主题配置文件 `~/.config/zcode-tui/config`(简单 `key = value` 行格式,
  无新依赖):9+2 个颜色 token 十六进制覆盖、`mouse = off`;
  `ZCODE_TUI_CONFIG` 覆盖路径;解析失败静默用默认值

明确不做(留待后批):todo 表视图(headless 场景尚未观察到数据)、
/commit //copy /export(输出消费批)、cell 级焦点导航(折叠先做简版)。

子进程说明:本批全部为进程内改动,无新子进程;不触碰现有进程组取消路径。

## Capabilities

### New Capabilities

- `session-picker`: /sessions 浮层列出并选择历史会话接续
- `persistent-history`: 内核历史读入 + Ctrl+R 反向搜索
- `mouse-scroll`: 滚轮回看 transcript 与开关
- `output-folding`: 长输出单元默认折叠与 Ctrl+O 展开
- `ui-config`: 用户配置文件(主题 token 覆盖、鼠标开关)

### Modified Capabilities

(无——不改变已归档能力的需求)

## Impact

- `src/lib.rs`:list_recent_sessions / recent_input_history / history_search /
  fold_preview / parse_ui_config 纯逻辑(全部单测)
- `src/main.rs`:鼠标捕获与事件、/sessions 与 Ctrl+R 浮层、折叠渲染与
  Ctrl+O、Theme 覆盖加载
- `tests/core.rs` 新单测;`tests/pty_smoke.py` 新场景(浮层、滚轮、配置)
- README / 设计文档 / CHANGELOG(0.3.0)
