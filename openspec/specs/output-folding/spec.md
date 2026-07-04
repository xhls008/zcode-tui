# output-folding Specification

## Purpose
TBD - created by archiving change session-picker-and-ui-comfort. Update Purpose after archive.
## Requirements
### Requirement: 长输出默认折叠
Tool/System/Diff/Error transcript cells longer than 24 lines SHALL
render folded by default: the first 8 lines plus a dim summary line
`… (+N lines · Ctrl+O)`. Assistant replies MUST never fold. Folding is
render-time only and MUST NOT mutate the stored cell text; the preview
computation MUST be a pure lib.rs function.

#### Scenario: 用户故事——长 diff 不再刷掉整屏
- **WHEN** `/diff` 输出 120 行
- **THEN** transcript 显示前 8 行 + `… (+112 lines · Ctrl+O)`,后续对话仍在一屏内可见

#### Scenario: 短输出不受影响
- **WHEN** 工具输出 10 行
- **THEN** 完整显示,无摘要行

### Requirement: Ctrl+O 展开/收起
`Ctrl+O` SHALL toggle the most recent foldable over-threshold cell
between folded and expanded, and the summary line MUST reflect the
hidden line count.

#### Scenario: 展开再收起
- **WHEN** 用户在折叠的 diff 后按 Ctrl+O,再按一次
- **THEN** 第一次完整显示 120 行,第二次回到 8 行预览

