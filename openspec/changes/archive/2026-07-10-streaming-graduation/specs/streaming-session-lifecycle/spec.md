# streaming-session-lifecycle

## ADDED Requirements

### Requirement: 流式路径的会话续接
系统 SHALL 在 app-server 路径的首条 prompt 前尊重待续接会话:`config.resume`
(来自 `--resume`、`/resume`、`/sessions` 选择)存在时,握手改发
`session/resume {sessionId}`(实测返回与 create 同形,含
messages/projection/session/todos)而非 `session/create`;resume 成功后该
sessionId 成为活跃会话,后续 prompt 复用。resume 失败(错误响应,如会话
不存在)MUST 提示并回退为新建会话,不阻塞当前 prompt。

#### Scenario: /sessions 选择后流式续接
- **WHEN** app-server 路径下用户经 /sessions 选中历史会话再提交 prompt
- **THEN** 握手走 session/resume 该 sessionId,回答在原会话上下文中生成(不再无声开新会话)

#### Scenario: resume 失败回退新建
- **WHEN** session/resume 返回错误(会话已不存在)
- **THEN** 提示失败,自动改走 session/create 新会话,prompt 正常完成

### Requirement: /sessions 流式数据源
app-server 连接活跃时,`/sessions` SHALL 用 `session/list {}`(空参,返回
sessions[]{sessionId,title,workspace,status,updatedAt})填充选择器,当前
workspace 的排前;连接不可用或列表为空时回退既有 db 轮询数据源。

#### Scenario: 协议数据源
- **WHEN** app-server 连接活跃,用户打开 /sessions
- **THEN** 列表来自 session/list,包含标题与相对时间,Enter 后按上一条 requirement 续接
