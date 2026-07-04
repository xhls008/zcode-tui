# tasks — session-picker-and-ui-comfort

## 1. lib.rs 纯逻辑

- [x] 1.1 list_recent_sessions:当前目录优先 + time_updated 降序 + LIMIT,relative age 格式化(验证:单测,临时 sqlite)
- [x] 1.2 recent_input_history:LIMIT 200 旧→新 + 相邻去重(验证:单测)
- [x] 1.3 history_search:子串匹配、新→旧序(验证:单测)
- [x] 1.4 fold_preview:阈值判断 + 预览行数 + 隐藏计数(验证:单测)
- [x] 1.5 parse_ui_config + parse_hex_color:行式解析、坏值/未知键忽略(验证:单测)

## 2. main.rs 接线

- [x] 2.1 配置加载:Theme 覆盖 + mouse 开关(NO_COLOR 仍最高优先)(验证:pty 冒烟)
- [x] 2.2 鼠标捕获:enter/drop/suspend 全生命周期,滚轮±3 行(验证:pty 冒烟发 SGR 序列)
- [x] 2.3 /sessions 浮层:列表渲染、↑↓/Enter/Esc、db 降级消息;命令目录+补全+palette 注册(验证:pty 冒烟)
- [x] 2.4 持久历史读入 + Ctrl+R 浮层(验证:pty 冒烟)
- [x] 2.5 折叠渲染 + Ctrl+O 切换 + help/README 快捷键表(验证:pty 冒烟)

## 3. 收尾

- [x] 3.1 门禁:fmt --check / clippy -D warnings / cargo test 全绿(验证:命令输出)
- [x] 3.2 pty 冒烟新场景对照 spec 用户故事跑通(验证:脚本输出)
- [x] 3.3 文档:README、设计文档、CHANGELOG 0.3.0 + Cargo.toml bump(验证:通读)

---

验证记录(2026-07-04):

- 门禁:fmt --check ✓、clippy -D warnings 零告警 ✓、51 个集成测试全绿(新增 6 个:
  会话列表排序/标题兜底、relative_age、历史读入去重、历史搜索、折叠阈值、配置解析)
- pty 冒烟 21/21(tests/pty_smoke.py,9 场景),新增场景逐条对照 spec 用户故事:
  s5=/sessions 浮层+Enter 接续(session-picker)、s6=Ctrl+R 取回内核持久历史
  (persistent-history)、s7=120 行输出折叠+Ctrl+O 展开(output-folding)、
  s8=accent 覆盖生效(ui-config)、s9=滚轮事件无崩溃(mouse-scroll);
  NO_COLOR/降级路径由 s3/s4 与单测覆盖
- 实现笔记:ratatui 单元格差量重绘导致与上帧共享前缀的状态文案只输出分歧尾部,
  冒烟断言须用后缀探针(已记入设计文档 §8)
