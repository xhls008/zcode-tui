# mouse-scroll

## ADDED Requirements

### Requirement: 滚轮回看
Mouse capture SHALL be enabled by default; wheel up/down scrolls the
transcript by 3 lines per notch using the existing scroll semantics
(lines back from the bottom; 0 follows the tail). Capture MUST be
released during suspend (/login, $EDITOR) and restored after, and MUST
be disabled entirely when `ZCODE_TUI_NO_MOUSE=1` or the config file sets
`mouse = off`.

#### Scenario: 用户故事——滚轮翻看长回复
- **WHEN** 助手输出超过一屏,用户向上滚动滚轮
- **THEN** transcript 向回滚动;滚回底部后恢复跟随最新输出

#### Scenario: 关闭开关
- **WHEN** `ZCODE_TUI_NO_MOUSE=1` 启动
- **THEN** 不启用鼠标捕获,终端原生选择/复制行为完全不受影响
