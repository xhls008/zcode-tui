# auth-experience

## ADDED Requirements

### Requirement: 认证检测三态
认证检测 MUST 区分三态:已配置(`~/.zcode/cli/config.json` 存在)、
部分配置(config.json 不存在但 env key 链任一存在)、未配置(两者皆无)。
部分配置态 MUST 提示内核仍需要模型配置及补齐命令,MUST NOT 显示为已认证。

#### Scenario: 仅有 env key
- **WHEN** ZCODE_API_KEY 已设但 config.json 不存在
- **THEN** /auth 与欢迎框显示"部分配置",附 `zcode login <plan>-api-key <key>` 补齐提示

#### Scenario: config.json 存在
- **WHEN** config.json 存在(无论 env key 是否设置)
- **THEN** 显示已配置,来源标注 config.json(env key 存在时一并打码显示)

### Requirement: 未登录启动屏
启动探测判定为未配置或部分配置时,TUI SHALL 在 transcript 顶部渲染
未登录屏:ZCODE 块字标使用 brand 紫 token 并带阴影层,底部一行
鸟巢/长城/天坛轮廓字符画,下方以 `›` 列出三条登录路径(/login、
`zcode login bigmodel-coding-plan-api-key <key>`、
`zcode login zai-coding-plan-api-key <key>`)。NO_COLOR/--no-color 时
MUST 全部退化为无色。已配置时 MUST NOT 显示该屏。

#### Scenario: 未配置启动
- **WHEN** 无 config.json 且无 env key,TUI 启动
- **THEN** 显示紫色带阴影字标 + 地标轮廓 + 三条登录路径;完成登录后 /auth 刷新,后续启动不再显示

#### Scenario: 无色模式
- **WHEN** NO_COLOR=1 下未配置启动
- **THEN** 字符画与文案照常显示但无任何颜色样式

### Requirement: /login 无桌面自动 --no-browser
The /login command SHALL append `--no-browser` when both `DISPLAY` and
`WAYLAND_DISPLAY` are unset/empty. The headless check MUST be a pure
lib.rs function (env snapshot as input, unit-testable). When
`ZCODE_TUI_LOGIN_CMD` overrides the command, nothing SHALL be injected.

#### Scenario: SSH 无桌面登录
- **WHEN** DISPLAY 与 WAYLAND_DISPLAY 均未设置,用户执行 /login
- **THEN** 实际执行 `zcode login --no-browser`,OAuth URL 打印在挂起的终端里

#### Scenario: 桌面环境登录
- **WHEN** DISPLAY 已设置
- **THEN** /login 执行 `zcode login`,不附加 --no-browser

### Requirement: brand 紫主题 token
Theme SHALL 新增 `brand` 与 `brand_dim` token(清华紫亮化变体与暗紫
阴影色),仅用于未登录屏与 logo 类元素;交互元素(›、链接、spinner、
行内代码)MUST 保持 GLM 蓝强调色不变。

#### Scenario: 强调色纪律不破
- **WHEN** 未登录屏与正常 transcript 同屏渲染
- **THEN** 紫色仅出现在字标/阴影/轮廓区域,其余强调元素仍为 GLM 蓝
