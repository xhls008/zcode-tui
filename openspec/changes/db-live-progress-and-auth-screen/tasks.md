# tasks — db-live-progress-and-auth-screen

## 1. 前置 spike 与依赖

- [x] 1.1 迷你 spike:全新目录首条 prompt,轮询 session/part 表钉死会话行出现时机,结论记入本文件本行下方(验证:spike 脚本输出)
  - 结论(2026-07-04 实测):session 行在 turn 开始 ~1.6s 即落库(总时长 16.3s),
    `directory = cwd` 精确匹配可用;该会话的 part 行随运行持续出现。
    首条 prompt 与 --continue 可统一为"按 directory 取最新会话 + rowid 基线排除旧行"。
- [x] 1.2 Cargo.toml 加 rusqlite(bundled),确认本地 release 构建与 cargo test 通过(验证:cargo build --release + cargo test)

## 2. db-consumer(lib.rs 纯逻辑)

- [x] 2.1 只读连接封装:mode=ro URI、busy_timeout、错误归一为"跳拍"类型(验证:单测,含文件不存在/坏文件路径)
- [x] 2.2 schema_migration 白名单校验函数(已知 id 集合子集判定)(验证:单测,覆盖子集/缺失/表不存在三态)
- [x] 2.3 会话解析:resume 显式 id / continue 按 directory 最新 / 首条基线快照(验证:单测,用临时 sqlite 造数)
- [x] 2.4 part/tool_usage 增量查询与类型化事件解析(text/reasoning/tool 状态/step 边界;未知类型安静忽略)(验证:单测,含真实 spike 采集的 data 样本)

## 3. prompt-json-result(lib.rs)

- [x] 3.1 --json 总结对象解析:response/sessionId/usage/contextUsed 提取,失败返回 None(验证:单测,真实总结对象样本 + 纯文本降级样本)
- [x] 3.2 prompt 命令构建附 --json;结果路由:解析成功走 markdown(response),失败走现有纯文本路径(验证:单测 + pty 冒烟)

## 4. live-progress(main.rs 接线)

- [x] 4.1 主循环分频轮询(每 5 拍),仅 prompt 任务运行且 db 未降级时启用;跳拍不打扰 UI(验证:pty 冒烟)
- [x] 4.2 工作区渲染:工具 chip(spinner→✓/✗+耗时)+ reasoning dim 单行(最新、截断);turn 结束整体清场(验证:pty 冒烟,真实内核跑多工具 prompt)
- [x] 4.3 状态栏上下文水位显示 + ≥80% 时 dim /new 建议(验证:单测格式化函数 + pty 冒烟)
- [x] 4.4 db 降级路径全程验证:改名 db 文件后 TUI 行为与现状完全一致(验证:pty 冒烟)

## 5. auth-experience

- [x] 5.1 认证检测三态改造(config.json 检查入链,lib.rs 签名扩展)(验证:单测,三态覆盖)
- [x] 5.2 /auth 与欢迎框文案更新(部分配置提示补齐命令)(验证:单测 + pty 冒烟)
- [x] 5.3 Theme 加 brand/brand_dim token,NO_COLOR 退化(验证:单测 token 映射)
- [x] 5.4 未登录屏字符画:清华紫字标+阴影层+底部鸟巢/长城/天坛轮廓+三条登录路径;仅未配置/部分配置显示(验证:pty 冒烟,含 NO_COLOR)
- [x] 5.5 /login 无桌面判定纯函数 + --no-browser 注入(LOGIN_CMD 覆盖时不注入)(验证:单测)

## 6. 收尾

- [x] 6.1 全量门禁:cargo fmt --check、clippy --all-targets --all-features -D warnings、cargo test 全绿(验证:命令输出)
- [x] 6.2 pty 冒烟全场景回归:流式进度、取消(进程组路径未动)、排队、降级、未登录屏(验证:冒烟脚本输出)
- [x] 6.3 文档同步:README(功能/命令/环境变量)、设计文档 §2.1/§3/§8 与 §5.1 待办勾销、CHANGELOG 新版本段(验证:通读一致性)

---

验证记录(2026-07-04):

- 门禁:cargo fmt --check ✓、clippy --all-targets --all-features -D warnings 零告警 ✓、
  45 个集成测试全绿 ✓(新增 8 个:认证三态×3、headless 判定、db schema/会话/增量、
  part 解析、summary 解析、--json 幂等、水位格式化)
- pty 冒烟 13/13(scratchpad/smoke.py,真实内核):实时工具 chip(✓ Read/Bash)、
  done 状态、ctx 水位、无 JSON 泄漏、response 渲染、Esc 取消、未登录屏
  (紫字标/轮廓/headline/登录路径/缺 db 不崩)、NO_COLOR 退化
- 4.4 降级:缺 db 场景由冒烟 s3 覆盖(fresh HOME),schema 不识别场景由单测覆盖
- 6.2 排队:排队逻辑本变更未触碰,沿用既有验证
- 实现中发现并修复:全新会话的 prompt 若按"目录最新"预解析会锁到旧会话,
  改为记 prior 会话并在轮询中排除(design.md D3 已按此落地)
