# check-zhipu-upload：只读本地证据检查

```bash
zcode check-zhipu-upload                 # 安装新 wrapper 后可用
zcode-tui check-zhipu-upload --json       # 独立二进制，无需安装官方应用
zcode-tui check-zhipu-upload --list-files # 明确允许显示已接受 manifest 中的文件名
zcode-tui check-zhipu-upload --home /absolute/home/copy
zcode-tui check-zhipu-upload --data-base-dir /absolute/custom/data
# TUI 内（本地命令，不会发给模型）
/check-zhipu-upload
```

**先停止官方桌面端再检查，可减少扫描期间状态变化。** 本命令不启动内核、不联网、
不获取令牌、不解密、不解包、不删除或锁定文件，也不读取工作区文件内容。
CLI 分派发生在配置加载和内核启动之前；TUI 命令通过本地子进程运行，保持界面响应。

## 检查范围

兼容已审计官方 Linux **3.11.2-6792 / 3.12.3-7463 / 3.14.0** 的状态结构；同结构的 macOS /
Windows 数据可读，但没有在这两平台做真实 Desktop 取证验证。

- 默认 HOME；`HOME/.zcode/v2/setting.json` 的 `dataBaseDir`；`ZCODE_DATA_BASE_DIR`；
  显式 `--data-base-dir`。所有发现的数据根均检查，而非仅检查当前最高优先级根。
- `--home` 隔离检查目标，不继承调用者的 `ZCODE_DATA_BASE_DIR`；复制的数据可通过
  `--data-base-dir` 补充。配置中的绝对路径仍可能指向原目录，应留意检查范围。
- 各根的 `.zcode/v2/checkpoints/*/state.json` 和旧 `repo-snapshots`；
  已接受 manifest；`pending/*.tar.gz.enc`、envelope 和 `tmp/*.tar.gz` 残留。
- 覆盖 3.12.3/3.14.0 的 `activeUpload` / `pendingUpload` 别名和
  `latestPendingUpload`；未知字段只作为覆盖警告，不作成功推断。
- 单个 JSON 最多 8 MiB、总 JSON 预算 64 MiB、目录条目预算 4096；错误或超限会提示
  coverage warning。显式数据根规范化后，拒绝其内部符号链接和 `..`，不跟随状态里
  指向外部的 manifest 路径。不是抵御并发恶意替换文件的取证沙箱。

## 结论解释

JSON `schema_version: 1` 的 `status` 是以下稳定枚举；`conclusion` 是双语说明。
同一 checkpoint 可以同时存在成功记录和新一轮待上传文件，分别显示，不互相覆盖。

| status | 含义 |
|---|---|
| `client_reported_accepted` | 存在 `lastAcceptedManifestHash` / extra 标记，按审计代码是客户端记录上传成功后的状态；不是独立的服务端收据 |
| `attempt_evidence` | 有尝试时间、次数或失败计数；不能当作成功 |
| `staged_evidence` | 有待处理状态或打包/加密残留；不能证明已经发送 |
| `no_local_evidence` | 本次所查位置没有上述证据；**不等于从未上传** |
| `inconclusive` | 未发现上述证据，但有读取/格式/覆盖问题，无法判断 |

即使 status 为成功或尝试，也必须检查 `warnings`，其他数据可能未覆盖。
默认匿名编号展示 checkpoint，不输出原始状态、凭据 handle、密钥或项目目录。
`--list-files` 只读安全位置中的已接受 manifest 文件名，不读实际文件；这些名字可能
涉及隐私，分享报告前请脱敏。manifest 是快照清单，不一定是某一次增量包的完整载荷，
也没有做密码学真实性验证。manifest 缺失或超限不抹掉已有成功状态，只提示清单不完整。

退出码 **0** 代表命令完成，不代表“安全”或“没有上传”；参数错误/致命错误为 **1**。
自动化应解析 JSON status 和 warnings，不应仅凭退出码放行。

不扫描任意日志、聊天数据库、系统其他用户、已删除文件或服务端；未保留状态、迁移目录、
其他上传机制、未来官方版本都可能超出范围。不要为验证此命令触发真实仓库上传。
