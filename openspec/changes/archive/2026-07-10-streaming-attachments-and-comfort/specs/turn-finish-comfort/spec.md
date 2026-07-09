# turn-finish-comfort

## ADDED Requirements

### Requirement: 长回合完成铃
流式回合或经典 assistant prompt 任务收尾且耗时超过 30 秒时,TUI SHALL
向终端写一个 BEL(`\x07`)提醒;被取消的回合 MUST 不响铃。配置文件
`notify = off` SHALL 关闭该行为(默认开启,沿 `mouse` 键的解析模式,
坏值忽略)。

#### Scenario: 长回合响铃
- **WHEN** 一个流式回合跑了 45s 后正常收尾,notify 未配置
- **THEN** stdout 收到一个 BEL

#### Scenario: 短回合与取消不打扰
- **WHEN** 回合 10s 完成,或用户 Esc 取消了一个 60s 的回合
- **THEN** 不发 BEL

#### Scenario: notify=off 静音
- **WHEN** 配置文件含 `notify = off`,长回合完成
- **THEN** 不发 BEL

### Requirement: 文件变更回合小结
流式回合期间 TUI SHALL 统计 `checkpoint.created` 事件(session/event
的 `params.type`,payload 携带 fileCount)的次数与 fileCount 总和;
回合收尾时若总和 >0,SHALL 追加一条 dim 系统行
`N file(s) changed · /diff to review`。无 checkpoint 的回合 MUST 不加行。

#### Scenario: 写文件回合给小结
- **WHEN** 一个回合内 2 个 Write 各触发一条 checkpoint.created(fileCount 各 1)
- **THEN** 收尾后 transcript 出现 `2 file(s) changed · /diff to review`

#### Scenario: 纯问答无小结
- **WHEN** 回合没有任何 checkpoint.created 事件
- **THEN** 不追加文件小结行
