# streaming-attachments

## ADDED Requirements

### Requirement: 流式路径 @文件 附件
app-server 流式路径的 `session/send` SHALL 把 prompt 中解析出的 `@路径`
提及(与经典路径同一 `extract_file_mentions` 越界防护:canonicalize、
仅 cwd 内普通文件)翻译为 `attachments[]`,每项按内核 schema `Pwt`
构造:扩展名 png/jpg/jpeg/gif/webp → `kind:"image"`(mimeType 对应
image/*),其余 → `kind:"file"`(mimeType 按扩展名,未知回退
text/plain,且 `sizeBytes` 必填);`filename` 取 basename,`localPath`
取绝对路径。握手完成后的首发 send 与会话复用的快路径 send MUST 都携带。
无 @提及时 MUST 不带 attachments 键(与现状全等)。

#### Scenario: 流式附件可被模型读到
- **WHEN** 流式路径下提交 `@notes.txt 里的暗语是什么`,notes.txt 含哨兵串
- **THEN** send 带 `attachments:[{kind:"file", filename:"notes.txt", mimeType:"text/plain", sizeBytes:N, localPath:<绝对路径>}]`,回答流回哨兵内容

#### Scenario: 图片扩展名走 image
- **WHEN** prompt 提及 `@shot.png`
- **THEN** 附件为 `{kind:"image", mimeType:"image/png", …}`(sizeBytes 可选但有值时如实)

#### Scenario: 读不到元数据宁缺毋滥
- **WHEN** 某提及文件在构造附件时 metadata 读取失败(竞态删除等)
- **THEN** 跳过该附件(不发半残对象触发内核 ZodError),其余附件与 prompt 正常发送
