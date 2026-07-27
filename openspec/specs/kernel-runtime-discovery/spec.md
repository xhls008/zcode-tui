# kernel-runtime-discovery Specification

## Purpose

Resolve the active ZCode desktop runtime and its update channel consistently
for system and rootless installations.

## Requirements

### Requirement: 活跃内核目录解析

系统 MUST 按 `ZCODE_APP` → `/opt/ZCode` →
`~/.local/opt/zcode/<version>/opt/ZCode` 的优先级定位含
`resources/glm/zcode.cjs` 的应用目录。多个 rootless 版本并存时 MUST 按
数字版本顺序选择最高版本，而不是字典序。wrapper 把控制权交给 fallback
TUI 时 MUST 通过 `ZCODE_APP` 传递其实际选择。

#### Acceptance Criteria

- Given `3.9.9` 与 `3.10.0` 同时存在, when 未显式设置 `ZCODE_APP`, then
  选择 `3.10.0`。
- Given wrapper 选择一个 rootless 应用目录, when 启动 fallback TUI, then
  子进程环境中的 `ZCODE_APP` 等于该目录。
- Given 显式 `ZCODE_APP` 有效, when 解析, then 不再被 `/opt` 或 rootless
  目录覆盖。

### Requirement: 更新源与已装版本对应活跃内核

启动探针和 `/update` MUST 从活跃应用目录读取 `resources/app-update.yml`,
并从 dpkg 或 rootless 路径取得已装桌面版本。显式
`ZCODE_TUI_UPDATE_FEED` MUST 优先；未显式覆盖时，若包内 feed 指向 loopback,
系统 MUST 使用项目记录的官方 Linux feed，避免把开发占位地址当更新服务。

#### Acceptance Criteria

- Given 活跃目录为 `~/.local/opt/zcode/3.3.6/opt/ZCode`, when 启动探针,
  then installed version 为 `3.3.6` 且读取该目录的 app-update.yml。
- Given 包内 URL 为 `http://localhost:8081`, when 无显式覆盖, then 选择官方
  HTTPS feed。
- Given `ZCODE_TUI_UPDATE_FEED=http://127.0.0.1:PORT/`, when 执行测试更新,
  then 使用该显式本地 feed。
- Given feed URL 含 shell 元字符, when 构造 `/update` job, then 其作为单个
  shell 参数处理，不执行额外命令。

### Requirement: 既有更新安全属性保持

适配后的 `/update` MUST 继续使用临时目录、deb 文件名 basename、sha512
校验和免密 sudo 探测；下载或校验阶段 Esc 取消 MUST 继续通过既有进程组
清理路径终止子进程。

#### Acceptance Criteria

- Given 下载 hash 不匹配, when `/update` 校验, then 删除 deb 且不调用 dpkg。
- Given 用户取消下载, when Esc, then 更新任务进程组被清理且 TUI 可继续使用。
