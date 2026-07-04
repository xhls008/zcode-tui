# ui-config Specification

## Purpose
TBD - created by archiving change session-picker-and-ui-comfort. Update Purpose after archive.
## Requirements
### Requirement: 配置文件加载
The TUI SHALL read `~/.config/zcode-tui/config` (path overridable via
`ZCODE_TUI_CONFIG`) at startup. The format is line-based `key = value`
with `#` comments. Supported keys: the theme tokens (`accent`,
`accent_dim`, `text`, `dim`, `good`, `bad`, `frame`, `code_bg`,
`band_bg`, `brand`, `brand_dim`) as `#rrggbb` colors, and `mouse`
(`on`/`off`). Unknown keys and malformed values MUST be ignored
silently; a missing or unreadable file MUST leave every default intact
— startup can never fail because of config. The parser MUST be a pure
lib.rs function.

#### Scenario: 用户故事——换掉强调色
- **WHEN** 配置文件写 `accent = #ff8800` 后启动
- **THEN** `›`、行内代码、spinner 等强调元素变为橙色,其余 token 保持默认

#### Scenario: 配置写坏了照常启动
- **WHEN** 文件里有 `accent = 不是颜色` 和未知键 `foo = bar`
- **THEN** TUI 正常启动,全部使用默认值,无报错

#### Scenario: NO_COLOR 优先级最高
- **WHEN** 配置了自定义颜色但设置了 NO_COLOR
- **THEN** 仍然全部无色

