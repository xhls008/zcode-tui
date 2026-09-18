# ZCode repository-snapshot privacy audit / 仓库快照隐私审计

Date: **2026-09-18**. Scope: official Linux x64 packages **3.11.2-6792** and
**3.12.3-7463**, and this project's managed CLI launcher. The privacy findings are from static analysis;
the new package was not installed. Its CLI was subsequently exercised only in
isolated compatibility tests with synthetic data and loopback model endpoints. No real repository
or account credentials were sent to an upload endpoint.

**结论：桌面 host 有完整的仓库快照加密上传链；已检查的 CLI 内核未发现同一
实现。本项目托管 CLI 路径不启动该 host，但不是禁网沙箱，也不保证模型请求
不会携带代码。这不是任何用户实际上传事件的取证结论。**

## 后续回应 / Reported response

[IT之家报道](https://www.ithome.com/1/004/310.htm)（[可读转载](https://www.aizws.net/news/detail/12690)）
转述智谱 9 月 18 日在官方群组致歉，称相关问题已修复；其解释是仓库索引/Wiki 功能
初期默认开启，Wiki 在云端生成，相关上传数据随后销毁，并宣布将开源、引入第三方审查。
这是**媒体转述的官方说法**，本项目没有独立验证修复生效范围、数据销毁或当前服务端策略。
以下审计和文末漫画针对上列哈希对应的历史构件，不应被读作“官方目前仍未处理”的结论。

## Sources and integrity

- [Official release notes](https://zcode.z.ai/en/changelog).
- [3.12.3 Linux x64 release manifest](https://cdn-zcode.z.ai/zcode/electron/releases/3.12.3/linux-x64/latest.yml).
- [Audited 3.12.3 DEB](https://cdn-zcode.z.ai/zcode/electron/releases/3.12.3/linux-x64/ZCode-3.12.3-linux-x64.deb).
- DEB size: **146906820 bytes**. SHA-512 matched the official manifest.
- The versioned manifest explicitly says “优化仓库快照上传的内存占用” /
  “Reduced memory usage when uploading repository snapshots”. This item was not
  present in the website's English release-note body when checked.

SHA-256 fingerprints (not CLI version strings):

| Artifact | SHA-256 |
|---|---|
| 3.12.3 DEB | `631fbd69fcefe5d57c607bbfd047bb7a474af6017464681b99ccb7b15749c60e` |
| 3.12.3 `resources/app.asar` | `1de494c4841a849b7f8cc8dc4df9e9ec861acf88cbc45f62bd087f2619a81bd1` |
| 3.12.3 ASAR `out/host/index.js` | `c8f7b2e50f2c8f7eeb030a377cfc4779b2a0e2037af2239e065157dc2e3e422e` |
| 3.12.3 `resources/glm/zcode.cjs` | `da61b0663336a65f7cce3dec223678794ccaa58158e304fc0d97b695434a8f01` |
| 3.11.2 ASAR `out/host/index.js` | `30911a90dadc5c384959d00d95ccc70c8cf38c74a9cb99c3168b0897d046d215` |
| 3.11.2 `resources/glm/zcode.cjs` | `e9f1868c0fdb863537ed910ee3828b9be96b8c2fd805473f63b439e1113266b8` |

## Verified Desktop data flow

```text
Conversation input accepted / other hooked task events
  → RepoSnapshotSidecarService
  → account token + server-issued upload credentials/public key
  → workspace scan + explicit .git enumeration + applicable extra inputs
  → baseline or incremental tar.gz
  → AES-256-CTR encryption; RSA-OAEP/SHA-256-wrapped data key
  → local pending queue
  → multipart POST to server-supplied OSS destination
  → accepted-manifest state and pending-artifact cleanup
```

The following readable symbol labels occur in the minified Desktop host bundle.
They are more useful for rechecking than formatter-dependent line numbers.

| Evidence labels | What the traced implementation does |
|---|---|
| `sendConversationCommandV4`, `reserveRepoSnapshotSidecar`, `captureRepoSnapshotSidecar` | Activates capture after accepted `sendText` or a `createSession` with first input. Other session/task paths also hook the sidecar. |
| `RepoSnapshotSidecarService`, `captureBeforePromptUnsafe` | Requires a token and an upload key before scanning, creating an encrypted artifact, registering pending state and flushing uploads. |
| `listGitVisibleFiles`, `appendRootGitMetadataPaths`, `walkGitMetadataFiles`, `scanRepoSnapshot` | Enumerates tracked and non-ignored untracked files, and separately includes root `.git` metadata. Git enumeration failure can fall back to a filesystem walk. |
| `shouldIncludeRepoSnapshotPathBeforeSample`, `shouldIncludeRepoSnapshotPath` | Excludes symlinks. Git internals pass before ordinary secret-name/size checks and are exempt from the binary-content filter. |
| `collectRepoSnapshotGlobalConfigs`, `sanitizeUnknown` | Collects applicable global configuration; redacts selected sensitive key names, not arbitrary secrets embedded in all strings. |
| `writeRepoSnapshotPlainArchive`, `createEncryptedRepoSnapshotArtifact`, `encryptArchive` | Archives files, prompt/manifest metadata and applicable extras; encrypts with a random 32-byte key and 16-byte counter, wraps the key with the server-provided public key, and normally removes temporary plaintext in `finally`. |
| `getRepoSnapshotArtifactPaths` | Uses `tmp/*.tar.gz`, `pending/*.tar.gz.enc`, envelopes and manifests under the configured data root's `.zcode/v2/checkpoints/<workspace-hash>/`. |
| `RepoSnapshotUploadClient`, `buildObjectUploadTarget`, `uploadPostObject` | Fetches `/api/v1/snapshot/upload-credential`; posts `repo-snapshot.tar.gz.enc` to `oss.host` / `oss.path` returned by the server, with an OSS callback carrying the wrapped key and attribution/checksum fields. |
| `RepoSnapshotUploadWorker`, `markAcceptedManifest` | Records upload acceptance and cleans corresponding pending artifacts after successful upload. |

Consequences and qualifications:

- This is **not `git push`**, and it is not merely a local rewind checkpoint.
- Ordinary `.env` filtering does not protect historical secrets inside Git
  objects. This is a collection-scope finding, not a claim that any particular
  user's history contained or transmitted secrets.
- If `.git` is a worktree pointer file, the inspected enumeration adds that
  file; it does not by itself demonstrate traversal of the external Git directory.
- The archive may also contain prompts, model/session metadata, applicable
  attachments and global configuration. It is not limited to a code diff.
- Encryption is not user-exclusive: the holder of the private key corresponding
  to the server-provided public key can recover the data key.
- The inspected pipeline does not read `optimizeAgentExperienceEnabled` or
  `repoSnapshotIndexingEnabled` as capture/upload gates. Their presence in a
  settings schema or in collected configuration is not proof of an opt-out.
- Upload still depends on server-issued credentials, quotas, network and other
  conditions. A nonempty `workspaceIdentity` skips the inspected capture entry;
  this does not establish the behavior of all remote deployments.
- Local ciphertext absence is not proof of no historical upload: successful
  uploads can clean it up. This public report contains no user's local logs,
  repository inventory, account identifiers or credentials.

## Why the managed TUI path is different

See [`install.sh`](../install.sh): the managed launcher invokes
`resources/glm/zcode.cjs` using Electron with `ELECTRON_RUN_AS_NODE=1`, or system
Node. It does not start the Desktop host. Browser Use's loader extracts the
matching Playwright runtime; it does not launch the Desktop host either.

Neither inspected `zcode.cjs` contained the identified snapshot endpoint,
`RepoSnapshotSidecarService`, archive name or this encryption implementation.
This project's TUI does not integrate that Desktop sidecar. This is a scoped
finding—not proof that every possible network transfer or dynamically loaded
plugin has been audited.

Cloud model requests can still carry file contents, tool output, diffs and
attachments. Tools and plugins can make their own network requests. This project
does not currently enforce OS-level filesystem/network isolation, cannot control
a separately running Desktop app, and cannot promise the same behavior after an
upstream update or a custom launcher substitution. A future official TUI selected
by the wrapper would need its own review.

## Rechecking safely

1. Download the exact version from the official release manifest and verify its
   hash. Extract the DEB without installing it or executing maintainer scripts.
2. Read/extract `resources/app.asar` as data. Inspect `out/host/index.js` and
   follow the symbol labels above from conversation entry to the actual network
   call; a keyword match alone is not a data-flow audit.
3. Compare the exact `resources/glm/zcode.cjs` with the fingerprints above and
   inspect the managed launch path. Recheck new versions independently.
4. Do not invoke upload APIs with real credentials or use a real repository to
   reproduce the behavior. Any dynamic validation should use isolated synthetic
   data and mock endpoints, with external network access blocked.

No server-side retention, downstream use, account eligibility, disclosure/consent
flow or legal compliance determination was established by this audit. The
3.12.3 static review is separate from this project's runtime compatibility tests.

## 本地自查

运行 `zcode-tui check-zhipu-upload`，详见[命令说明与证据分级](check-zhipu-upload.md)。
它无需官方内核、不联网；客户端记录成功不等于独立的服务端确认，未发现不等于从未上传。

## 讽刺漫画：到底在防谁？

选取 [ferstar 2026-09-17 的公开评论](https://blog.ferstar.org/posts/zcode-silent-workspace-snapshot-upload/)
中“连你自己和客户端本体都解不开”这一讽刺点，改编为原创漫画。
这是一种编辑选择，不宣称客观评选了“全网最讽刺”；漫画对白是创作，不是网友原话。
只借鉴这个评论点，不背书原文对所有用户、用途或法律责任的其他推断。

![三格讽刺漫画：用户请 AI 改一行代码，机器人打包 Git 历史并说已加密；用户问钥匙，钥匙却在云端，连文件主人也打不开。](../output/imagegen/privacy-comic.png)

艺术化表达针对已审计历史版本的密钥控制问题，不是当前事件照片或新的取证材料。
使用用户授权的 imagegen API/CLI（gpt-image-2）生成；
[完整提示词](../output/imagegen/privacy-comic.prompt.txt)随仓库保存，不包含密钥或接口配置。
