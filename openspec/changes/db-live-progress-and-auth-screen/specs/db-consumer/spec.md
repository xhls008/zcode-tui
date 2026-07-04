# db-consumer

## ADDED Requirements

### Requirement: 只读访问内核数据库
TUI 对 `~/.zcode/cli/db/db.sqlite` 的所有访问 MUST 使用只读连接
(`mode=ro` URI + busy_timeout),MUST NOT 执行任何写操作。

#### Scenario: 内核正在写入时读取
- **WHEN** 内核进程正在向 db.sqlite 写入 turn 数据,TUI 同时发起只读查询
- **THEN** 查询正常返回或因 busy 失败;失败时该拍被跳过,不重试、不报错到 UI

### Requirement: schema 白名单校验与降级
TUI MUST 在启动探测阶段读取 `schema_migration` 表,与实现时点的已知迁移
id 集合比对:已知集合是实际集合的子集则启用 db 功能;否则(缺任何已知
迁移、表不存在、文件不存在、任何读取异常)MUST 将全部 db 衍生功能降级
隐藏,UI 行为与 db 功能不存在时完全一致。

#### Scenario: 内核升级新增迁移
- **WHEN** db 的 schema_migration 包含全部已知 id 及若干新增 id
- **THEN** db 功能正常启用

#### Scenario: schema 不识别
- **WHEN** schema_migration 缺少任一已知迁移 id,或表/文件不存在
- **THEN** 所有 db 衍生功能隐藏,prompt 任务回到纯 spinner 等待行为,无错误提示刷屏(至多一条 dim 系统消息)

### Requirement: 当前会话解析
TUI MUST 能在 prompt 子进程运行期间确定其归属会话:`--resume` 时使用
显式 sessionId;`--continue` 时 spawn 前按 `session.directory = cwd`
取 `time_updated` 最新的会话;首条 prompt 时记录 spawn 前的
`MAX(part.rowid)` 基线,轮询时仅归属基线之后且 directory 匹配的行。

#### Scenario: continue 模式锁定会话
- **WHEN** TUI 以 --continue 提交 prompt 且 cwd 存在历史会话
- **THEN** 轮询查询以该会话 id 过滤,不混入其他目录的会话数据

#### Scenario: 全新目录首条 prompt
- **WHEN** cwd 从未有过会话,首条 prompt 运行中
- **THEN** 基线快照法保证只显示本次运行新增的 part 行;会话行尚未出现的拍次安静跳过

### Requirement: 增量事件查询
TUI SHALL 提供按会话与 rowid 基线查询 `part` 增量行、按会话查询
`tool_usage` 状态的纯逻辑接口(lib.rs,可单测),返回类型化的事件
(text/reasoning/tool 状态/step 边界)。

#### Scenario: 解析 tool part
- **WHEN** part.data 为 `{"type":"tool","tool":"Read","state":{"status":"completed"},...}`
- **THEN** 接口返回带工具名与完成状态的类型化事件

#### Scenario: 未知 part 类型
- **WHEN** part.data 的 type 是未见过的值或 JSON 结构异常
- **THEN** 该行被安静忽略,不中断整批解析
