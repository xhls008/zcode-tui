# Browser Use：支持范围与验证

## 使用方式

从源码构建后，运行 `./install.sh` 更新 **TUI、zcode wrapper 和运行时 helper**。
只替换 TUI 二进制不会更新旧 wrapper。安装目录默认是 `~/.local`，不修改 `/opt/ZCode`。

```bash
zcode tui --browser-use headless --browser-executable /usr/bin/google-chrome
```

省略 `--browser-executable` 时由官方内核选择受支持的 Chrome/Chromium；TUI 检测到
系统候选路径不代表浏览器已经启动，也不会自动下载浏览器。

**权限边界：** 浏览器任务默认显式传入 `--mode build`，避免官方经典 CLI 默认的
`yolo` 模式被隐式启用。经典 CLI 没有交互式权限确认客户端，需要确认的工具会被
拒绝；需要逐次审批时应使用官方桌面端。只有用户明确选择 `--mode yolo` 时才透传
该模式；它允许工具免确认执行，不适合不受信任页面或任务，TUI 不会自动开启。

3.11.2 的 CLI help 宣传了 `--allowed-tools`，但实测解析器返回 Unknown option。
TUI 不会为“让它能跑”而丢弃用户的 allowlist，也不会改 sandbox 启动参数。

## 已接入

- 校验显式浏览器路径：必须是绝对路径、存在的普通文件，Unix 下需有执行位。
- 显示经典路由限制、等待模型/工具、工具运行状态、已用时间与取消入口。
- 内核数据库首次创建后也能接续只读进度；数据库不可用时保留计时与取消，不伪造进度。
- 从当前任务的工具结果和 stderr 识别运行时缺失、浏览器启动失败、权限拒绝等问题。
  错误会保留在 transcript；CLI exit 0 不被当作浏览器操作成功的证据。
- 普通模型输出继续通过官方 summary 处理，旧 3.11.2 的 app-server 对话路径不改变；3.12.3 全部普通对话也使用经典兼容路由。

## 3.11.2 打包缺口的兼容处理

官方 `resources/glm/zcode.cjs` 会导入 `playwright-core`，但 Linux 桌面包将其放在
`resources/app.asar` 中，独立 CLI 无法直接找到它。未修复时，实际浏览器工具返回：

```text
Managed headless Chromium is unavailable: failed to load the pinned Playwright runtime.
```

托管 wrapper 仅在 Browser Use 调用中加载 `lib/zcode-tui/browser-runtime.mjs`：

1. 优先保留原有模块解析；已有可用模块时不覆盖。
2. 缺少 `playwright-core` 时，从**同一官方 app.asar**提取该包到
   `${XDG_CACHE_HOME:-$HOME/.cache}/zcode-tui/browser-runtime/`。
3. 缓存按源路径及 ASAR header 的 SHA-256 区分，校验包中提供的文件 SHA-256，
   新官方包不会误用旧缓存；提取完成前不会发布缓存目录。
4. 不联网安装 npm 包、不更改官方内核、不设置全局 `NODE_OPTIONS`，不读取个人浏览器资料。

`ZCODE_TUI_BROWSER_RUNTIME=off` 可关闭兼容桥，供排障或比较官方原始行为。
`install.sh --uninstall` 删除受托管 helper；缓存可按上述独立目录手动清理。

## 不在本次范围

- 桌面内嵌浏览器窗口、扩展接管个人 Chrome、录像 UI、Computer Use 桌面操作。
- 浏览器任务的 app-server token 流式、途中 steer 和交互审批。
- 跨 CLI 进程保持同一浏览器标签页、真实账户登录站点和付费模型任务。
- Windows/macOS 的真实浏览器端到端验证；本次实测平台是 Linux x64。

临时 HOME/浏览器资料隔离不等于操作系统安全沙箱；浏览器启动策略仍由官方内核与
配套 Playwright 决定。处理不受信任页面时应另行使用受隔离的运行环境。

## 可重跑验证

```bash
cargo test --locked
cargo build --release --locked
python3 tests/browser_runtime_test.py
# 仅测试依赖；建议装在临时 venv 中，与既有 PTY 测试一致
python3 -m pip install pyte==0.8.2
python3 tests/browser_smoke.py

# 显式 opt-in：真实官方内核、Chrome；模型与页面均为本机临时 HTTP 服务
python3 tests/browser_smoke.py --official \
  --kernel /opt/ZCode/resources/glm/zcode.cjs \
  --browser /usr/bin/google-chrome
```

默认检查不需要账户、官方内核或 Chrome；CI 和 Release 质量门禁执行它们。
真实测试使用独立 HOME/XDG/TMPDIR 和工作目录，清空继承的认证环境变量，模型只向
loopback 请求。成功用例明确使用 yolo，但模拟模型只发出测试代码里的固定浏览器操作。
不因模拟模型说“成功”就判定通过：必须在真实 DOM 中读到随机校验值、点击按钮并
观察 DOM 改变及本地服务收到点击请求。另覆盖权限拒绝、禁用运行时兼容桥、取消，
Linux 下检查该临时目录所属的浏览器/内核进程没有残留。

验证基线（2026-09-16）：ZCode **3.11.2-6792 / CLI 0.16.5**、包内 Playwright
**1.59.1**、Chrome **147.0.7727.101**；系统 Node **22.22.0**、Electron 内嵌
Node **24.14.0**。官方内核 SHA-256：

```text
e9f1868c0fdb863537ed910ee3828b9be96b8c2fd805473f63b439e1113266b8
```

### 3.12.3 追加验证（2026-09-18）

**3.12.3-7463** 已在仅启用 loopback 的 Linux 网络命名空间中通过同样五项真实浏览器
用例（系统 Node、Electron、缺失运行时、权限拒绝、取消），没有用真实账号或远端模型。
从同包 ASAR 提取运行时仍有效；wrapper 同时修复新版 bundled provider 配置文件路径。
官方 CLI 仍报 0.16.5，但 app-server 已移除旧 registry 接口，不能据版本号复用旧协议。
本版本主动使用经典 CLI；暂不提供 V4/途中 steer/会话内模型切换，`/model` 明确提示而
不假装切换成功。原生 provider 配置与旧 CLI 配置迁移由官方经典 CLI 管理，不改写
官方包或代替它迁移用户凭据。更新时必须同时更新 wrapper（重新运行 install.sh）。
