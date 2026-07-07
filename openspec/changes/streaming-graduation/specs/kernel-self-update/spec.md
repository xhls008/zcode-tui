# kernel-self-update

## ADDED Requirements

### Requirement: /update 内核自更新
`/update` SHALL 复用启动探针的官方渠道(app-update.yml → latest-linux.yml):
无新版时提示已最新;有新版时下载 deb 到临时目录、按 feed 内 sha512
(base64)校验,校验失败 MUST 中止并删除文件。校验通过后:免密 sudo 可用
(`sudo -n true` 成功)则执行 `sudo -n dpkg -i` 并提示重启 TUI 生效;
不可用则打印免 root 解包指引(`dpkg-deb -x` 到
`~/.local/opt/zcode/<ver>/`,wrapper 自动探测)。下载/安装期间 UI 不阻塞
(后台任务 + 状态行进度),Esc 可取消下载。

#### Scenario: 有新版且免密 sudo
- **WHEN** feed 版本高于已装版本,用户执行 /update,sudo -n 可用
- **THEN** 下载→sha512 通过→dpkg 安装成功→提示新版本号与重启建议

#### Scenario: 校验失败中止
- **WHEN** 下载文件的 sha512 与 feed 不符(截断/篡改)
- **THEN** 中止安装、删除下载文件、提示重试;系统不变

#### Scenario: 已是最新
- **WHEN** feed 版本不高于已装版本
- **THEN** 提示已是最新,不下载

### Requirement: 更新 Tip 引导 /update
启动更新提示的 Tip 文案 SHALL 包含 `/update`,替代手动下载指引的首选位置
(deb 直链与 changelog 链接保留)。

#### Scenario: Tip 含 /update
- **WHEN** 启动探针发现新版
- **THEN** Tip 首行给出 /update,一步完成升级
