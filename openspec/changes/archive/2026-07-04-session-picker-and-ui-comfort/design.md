# design — session-picker-and-ui-comfort

## Context

db 消费地基(已归档变更)提供只读连接、schema 白名单与降级纪律;
浮层机制已有两个先例(补全菜单、命令面板)可复用交互模式;
Theme token 体系就位。约束不变:lib.rs 纯逻辑可单测,浮层是唯一
带边框元素,GLM 蓝单一强调色。

## Goals / Non-Goals

**Goals:** 会话可发现可接续;历史跨进程可搜;滚轮回看;长输出不刷屏;
颜色可个性化。全部改动在 db 降级 / 无配置文件时零影响。

**Non-Goals:** cell 焦点导航与逐单元折叠(简版只折叠+Ctrl+O 最近单元);
todo 视图;写任何 db 数据;TOML 依赖。

## Decisions

**D1. `/sessions` 新命令而非改 `/resume` 语义。** `/resume`(裸)= --continue
的既有契约保留;选择器是加法。选择后走现有 `set_resume` 路径(下一条
prompt 生效),不重启会话进程——与内核 headless 模型一致。

**D2. 会话列表查询排序:当前目录的会话排前,组内按 time_updated 降序,
LIMIT 20。** 标题缺省时显示目录尾段;时间显示相对值(纯函数格式化)。

**D3. 历史合并策略:启动读 `input_history`(LIMIT 200,旧→新)作为底层,
本进程提交追加其上;相邻去重。** 不写回 db(内核自己会记录 --prompt)。
Ctrl+R 为子串匹配、新→旧,输入即过滤,浮层复用补全菜单的渲染模式。

**D4. 鼠标捕获默认开,两级退出。** `ZCODE_TUI_NO_MOUSE=1` 或配置
`mouse = off` 不启用;捕获时终端原生选择需按住 Shift(README 说明)。
滚轮一格 = 3 行,复用现有 scroll(距底行数)语义;suspend(/login、
$EDITOR)时随 raw mode 一起释放/恢复。

**D5. 折叠是渲染期行为,不改 LogLine 数据。** 阈值 24 行;可折叠 kind:
Tool/System/Diff/Error(assistant 回复永不折叠);折叠显示头 8 行 +
dim 摘要行 `… (+N lines · Ctrl+O)`。折叠状态存 UiState 的
`unfolded: HashSet<usize>`(log 索引),Ctrl+O 切换最近一个可折叠且
超阈值的单元。fold_preview 为 lib.rs 纯函数(输入文本与阈值,输出
预览行数或 None)。

**D6. 配置文件用行式 `key = value`,自研 30 行解析器,不引 TOML 依赖。**
键:9 个既有 token + brand/brand_dim(值 `#rrggbb`)、`mouse`(on/off)。
未知键忽略、坏值忽略(用默认),解析永不失败——配置坏了 TUI 必须照常起。
路径 `~/.config/zcode-tui/config`,`ZCODE_TUI_CONFIG` 覆盖。

## Risks / Trade-offs

- [鼠标捕获影响习惯选择] → 默认开但文档醒目说明 Shift 旁路 + 两级关闭
- [折叠隐藏关键错误细节] → Error 折叠阈值同样 24 行,且摘要行显示隐藏行数
- [input_history 表结构变化] → 查询走 db 模块同一降级纪律,失败=无持久历史
- [会话列表过长/标题缺失] → LIMIT 20 + 目录尾段兜底

## Open Questions

(无——全部决策可直接实现;todo 视图数据形态待后批观察。)
