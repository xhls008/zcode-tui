# zcode-tui

[English](README.en.md) · [Releases](https://github.com/xhls008/zcode-tui/releases) · [设计文档](docs/2026-07-04-design.md)

![zcode-tui](assets/zcode-tui-auenger.png)

> 非官方社区项目：为缺少可用 `@zcode/tui` 的 ZCode Linux 安装补上一个
> SSH/tmux 友好的终端界面。它运行官方 CLI 内核，不替代官方桌面端。

`zcode-tui` 是 Rust + Ratatui 编写的终端 TUI。普通输入优先使用官方
`app-server` 流式协议；协议不可用时自动回退到 `zcode --prompt`。

## 快速开始

### Linux：下载 Release（无需 Rust）

```bash
mkdir -p ~/.local/bin
curl -fL -o ~/.local/bin/zcode-tui \
  https://github.com/xhls008/zcode-tui/releases/latest/download/zcode-tui-x86_64-unknown-linux-musl
chmod +x ~/.local/bin/zcode-tui

# 需要 `zcode` wrapper 时：
curl -fLO https://github.com/xhls008/zcode-tui/releases/latest/download/install.sh
bash install.sh --no-build
```

### 从源码安装

```bash
./install.sh                 # 构建并安装 TUI + zcode wrapper
./install.sh --no-wrapper    # 只安装二进制
./install.sh --uninstall
```

首次使用前登录官方内核：`zcode login`，或在 TUI 中执行 `/login`。
服务器没有桌面时，需 Node ≥ 22.5；官方 Electron 可运行时则无需单独安装 Node。

## 为什么需要它

官方 Linux 包曾在帮助中提供 `tui` 入口，却没有交付可用的 `@zcode/tui`。
本项目只做终端壳和兼容层，让 SSH、tmux、无桌面服务器可以直接工作；不声称拥有
官方桌面端的全部能力。

![ZCode 发布了，TUI 呢？](assets/zcode-no-tui-satire.png)

*产品力讽刺：菜单上写着 TUI，用户得到的却是缺失的包。*

![菜单写着牛肉面，端上来一碗热水](assets/beef-noodle-hot-water-satire.png)

*产品力讽刺：宣传入口和实际交付之间，不该由用户自己补完。*

## 隐私边界

### 本项目做什么

- 托管路径直接运行 `resources/glm/zcode.cjs`，不启动桌面端
  `app.asar/out/host/index.js`。
- 已审计的官方 CLI 内核（3.11.2、3.12.3）未发现桌面端仓库快照上传实现。
- `check-zhipu-upload` 是本地只读检查，不启动内核、不联网、不获取凭据、不解密或删除文件。

### 本项目不保证什么

- 模型请求、附件、工具、MCP、插件和 Browser Use 仍可能联网并发送内容。
- 这不是文件系统或网络隔离沙箱，也不能约束另行运行的官方桌面应用。
- “未发现本地证据”不等于“从未上传”；客户端成功状态也不是独立的服务端收据。

### 对官方隐私设计的批评

针对已审计的历史桌面包，审计发现官方 host 会在会话输入后触发仓库快照流程：枚举
工作区和部分 `.git` 元数据，打包、加密，再上传到服务端提供的对象存储地址。数据密钥
由服务端公钥封装，用户并不持有对应私钥；因此“已加密”不等于“用户可控制”或“只有用户
能解密”。审计还没有看到明确的逐次授权界面，配置中的索引/体验开关也不能作为可靠的
退出上传证明。

这项批评针对上述固定版本的透明度、默认行为和用户控制权，不断言每位用户都发生过上传，
也不作法律定性。官方后续曾公开致歉并声称修复、开源和引入第三方审查；修复范围和当前
服务端策略仍应由用户按版本重新核验，详见[审计记录](docs/privacy-audit-2026-09-18.md)。

### 新增隐私监测功能

本项目新增 `check-zhipu-upload`，用于在本机检查官方数据目录留下的上传相关证据：

- 只读、离线运行，不启动官方内核、不读取工作区内容、不获取凭据、不解密或删除文件；
- 检查已接受 manifest、尝试/失败计数、待处理或加密残留，并覆盖 3.12.3 的上传状态字段；
- 输出 `client_reported_accepted`、`attempt_evidence`、`staged_evidence`、
  `no_local_evidence`、`inconclusive` 五类状态，可用 `--json` 接入自动化审计；
- 会报告读取错误、超限和覆盖不足。`no_local_evidence` 只表示本地未找到证据，不能证明
  从未上传，也不是服务端回执。检查前建议先退出官方桌面端。

```bash
zcode check-zhipu-upload                 # wrapper 或独立检查
zcode-tui check-zhipu-upload --json       # 机器可读结果
# TUI 内：/check-zhipu-upload
```

检查范围、证据分级和隐私注意事项见
[check-zhipu-upload 文档](docs/check-zhipu-upload.md)。固定版本的代码审计见
[隐私审计记录](docs/privacy-audit-2026-09-18.md)。

**立场：**反对未充分告知、未经明确授权的仓库后台采集；“已加密”不能代替用户的知情和控制。
该批评针对可核查的历史构件，不对所有用户的实际上传、用途或法律责任作未经证实的断言。

![三格讽刺漫画：用户请 AI 改一行代码，云端机器人却独自持有解密钥匙。](output/imagegen/privacy-comic.png)

*漫画根据 [ferstar 的公开评论](https://blog.ferstar.org/posts/zcode-silent-workspace-snapshot-upload/)
的讽刺点原创改编；对白不是网友原话，也不是当前服务端状态证明。*

## 兼容性

| 组件 | 状态 |
|---|---|
| 官方桌面包 | 3.12.3-7463、3.11.2-6792 已审计 |
| 3.12.3 | 经典 CLI 创建/连续恢复；暂不支持 app-server/V4/会话内模型切换 |
| 3.11.2 | app-server 流式、V4 控制、取消和恢复已验证 |
| Browser Use | 3.11.2/3.12.3 使用官方经典 CLI 路径；不提供途中 steer 或交互审批 |
| 发布版本 | `zcode-tui 0.7.0` |

内核版本号相同不代表协议相同；更新官方包后应重新验证。Release 包含 Linux、Windows、
macOS Intel 和 Apple Silicon 二进制，并提供 `SHA256SUMS`。

## 功能概览

- 多轮会话、`/new`、`/resume`、`/sessions`、权限模式和实时上下文/token 状态。
- app-server 阶段流式、工具权限确认、失败自动回退到经典 CLI。
- MCP（项目级/用户级，stdio/http/sse）、`@file` 附件、shell escape 和 git diff。
- `/agents` 只读 Agent Inspector、后台任务取消、`/rewind` 检查点回滚。
- Markdown/代码高亮、CJK 宽度处理、主题、自定义主题、OSC52 复制、输入历史。
- Browser Use headless 路径和官方 Playwright 运行时桥接；详见
  [Browser Use 文档](docs/browser-use.md)。

## 常用命令

| 命令 | 作用 |
|---|---|
| `text` | 发送 prompt；优先流式，失败回退经典 CLI |
| `@path` | 附加当前工作区内文件；拒绝越界和符号链接逃逸 |
| `! <cmd>` | 执行本地 shell 命令 |
| `/login`、`/logout`、`/auth` | 登录、登出、查看认证状态 |
| `/status` | 会话、认证和 MCP 概览 |
| `/mcp ...` | 管理 MCP server |
| `/mode [build\|edit\|plan\|yolo]` | 查看或切换权限模式 |
| `/model`、`/think`、`/compact` | 流式会话控制（3.12.3 经典路径除外） |
| `/usage [7d\|30d]` | token 用量 |
| `/agents` | 只读查看 Agent/Background |
| `/rewind` | 预览并回滚检查点 |
| `/diff [args]` | 高亮 `git diff` |
| `/theme [name]` | 查看或切换主题 |
| `/update` | SHA-512 校验后更新官方内核 |
| `/check-zhipu-upload` | 本地上传证据检查 |
| `/copy`、`/editor`、`/clear`、`/new`、`/exit` | 复制、编辑、清屏、重开、退出 |

完整命令列表可在程序中运行 `zcode-tui --help`；MCP 和上传检查的参数见对应文档。

## 配置

配置文件：`~/.config/zcode-tui/config`（`key = value`）。常用示例：

```text
theme = dark
notify = off                 # 关闭超过 30 秒回合的终端铃
accent = #6088ff             # 主题 token 覆盖
```

| 变量 | 用途 |
|---|---|
| `ZCODE_TUI_ZCODE_BIN` | 指定官方 `zcode` 路径 |
| `ZCODE_APP` | 指定官方桌面包目录 |
| `ZCODE_FALLBACK_TUI` | 指定 fallback TUI 二进制 |
| `ZCODE_TUI_APP_SERVER=0` | 强制经典 `--prompt` 路径 |
| `ZCODE_FORCE_SYSTEM_NODE=1` | 不使用 Electron 内嵌 Node |
| `ZCODE_TUI_NO_UPDATE_CHECK=1` | 关闭启动更新检查 |
| `ZCODE_TUI_CONFIG` | 指定 TUI 配置文件 |
| `ZCODE_TUI_LOG` | 开启不记录参数/凭据的协议摘要日志 |

自定义主题、完整环境变量和 wrapper 行为见 `zcode-tui --help`、
[构建文档](BUILDING.md)及 [设计文档](docs/2026-07-04-design.md)。

## 限制

本项目只提供终端壳和兼容层，不重建官方桌面端或缺失的官方 `@zcode/tui`。
工具权限不能替代操作系统隔离；处理敏感项目时请限制工作区、网络出口和 MCP/插件权限。

## 开发

```bash
cargo fmt --check
cargo test --locked
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo build --release --locked
```

真实内核和 Browser Use 测试使用临时 HOME、合成数据与 loopback 服务，详见 `BUILDING.md`。

## License

MIT
