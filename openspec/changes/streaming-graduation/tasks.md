# streaming-graduation · tasks

## 1. lib 纯函数层

- [x] 1.1 `app_server_enabled` 默认反转(未设/非关闭值→true;0/off/false/no→false,大小写不敏感)——单测:默认开、各关闭值、=1 兼容
- [x] 1.2 `app_resume_params(session_id)`、`app_usage_params(session_id)`、`usage_stats_params(range)` + `parse_session_list(result) -> Vec<SessionRow>`(复用既有 SessionRow,协议字段映射 title/directory/time)——单测:实测抓取的 list 结果解析
- [x] 1.3 `parse_steer_result(&Value) -> SteerOutcome {Queued, Rejected(String), Unknown}`——单测:queued/rejected(带 reason)/未知形状
- [x] 1.4 `parse_kernel_slash_commands(result) -> Vec<KernelCommand{name,description,input_hint}>` + slash_suggestions 支持动态命令合并(本地优先去重)——单测:合并、去重、inputHint 展示
- [x] 1.5 `parse_todos(result_or_patch) -> Vec<TodoItem{text,done}>`(create/resume 结果与 state 推送两处形状)——单测
- [x] 1.6 update feed 解析扩展:`parse_update_feed` 增加 deb 的 sha512(base64)提取——单测:真实 feed 样本

## 2. 流式会话续接

- [x] 2.1 握手状态机加 Resume 分支:config.resume 存在时首请求 session/resume;成功→同 Create 流程(sessionId→subscribe);错误→提示一条 dim + 改发 create 重走(不 downgrade)
- [x] 2.2 /sessions 数据源:app_conn 活跃时 request_blocking("session/list", 3s) 填充选择器(当前 workspace 排前,running 会话标注),失败回退 db;选择写 config.resume(两路径共享)
- [x] 2.3 resume 成功后清 config.resume(一次性),欢迎框会话状态刷新

## 3. 默认开启毕业

- [x] 3.1 冒烟经典场景显式 ZCODE_TUI_APP_SERVER=0(s1/s2/s7 等假定 --prompt 的);s10/s12-16 去掉显式 =1(验证默认即流式)
- [x] 3.2 README(功能段、环境变量段、安装段)与 /help、CHANGELOG:默认开、=0 退出、降级纪律不变

## 4. steer 被拒处理

- [x] 4.1 pump 的 Response 成功分支:ControlReq::Steer 时 parse_steer_result;Rejected→按 reason 提示 + 输入退回排队;Queued→静默——单测(lib 部分);冒烟:紧跟回合末尾发 steer 观察退队(时序难稳定则以单测+代码审查为准)

## 5. /update 自更新

- [x] 5.1 /update 命令:探针逻辑复用(feed 读取+版本比较);无新版提示;有新版走 job 基建下载(curl --retry,transcript 进度可折叠、Esc 取消)
- [x] 5.2 下载完成回调:openssl dgst sha512 校验(base64 对比,失败删文件中止);`sudo -n true` 探测→可用则 `sudo -n dpkg -i`(输出落 transcript)+ 重启提示;不可用打印免 root 解包指引
- [x] 5.3 更新 Tip 文案加 /update;slash 目录/palette/help 收录——冒烟:s18 伪 feed(本地 http)下 /update 走到 sha512 校验失败中止路径(不真装)
- [x] 5.4 /update 进行中互斥(is_busy 纳入),完成后建议 /exit 重启

## 6. /usage 与 todos 与补全

- [x] 6.1 /usage [7d|30d]:session/usage + usage/stats 并发发出,结果渲染 System 条目(token 细分表 + 汇总行含 cacheHitRate);无会话提示
- [x] 6.2 todos 缓存 + 工作区渲染(非空才显示,状态符号);create/resume 结果与 state 推送两处更新
- [x] 6.3 内核 slashCommands 缓存(create/resume 结果),slash 补全动态合并
- [x] 6.4 冒烟:s19 /usage 输出包含 totalTokens 与 7d 汇总(screen/plain)

## 7. 收尾与发版 0.5.0

- [x] 7.1 全量门禁:fmt/clippy -D warnings/test/pty 冒烟全绿;install.sh 部署
- [x] 7.2 CHANGELOG [Unreleased] → [0.5.0],Cargo.toml bump 0.5.0,README 版本引用核对
- [x] 7.3 提交(逻辑分组)+ 打 tag v0.5.0(push 与 Release CI 由用户执行)
