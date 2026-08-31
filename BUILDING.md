# 构建与打包

本文说明如何在当前 Intel macOS 环境构建可执行文件，以及如何生成项目正式发布所用的 Linux x86_64 musl 静态产物。

## 环境要求

- Rust stable，建议使用项目当前验证过的 Rust 1.93 或更新版本。
- 从仓库根目录执行所有命令。
- 使用 `Cargo.lock` 中锁定的依赖版本，不要在发布构建中省略 `--locked`。

确认环境：

```sh
cd /Users/ryan/mycode/zcode-tui
rustc --version
cargo --version
```

首次联网下载依赖：

```sh
cargo fetch --locked
```

如果 Cargo 在当前网络下更新索引较慢，可使用本机已验证的连接参数：

```sh
CARGO_REGISTRIES_CRATES_IO_PROTOCOL=sparse \
CARGO_HTTP_MULTIPLEXING=false \
CARGO_HTTP_TIMEOUT=60 \
cargo fetch --locked
```

依赖进入本地缓存后，可以给后续命令加 `--offline`。

## 当前 macOS 构建

先运行项目质量检查：

```sh
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --locked
```

构建当前机器可运行的 release 二进制：

```sh
cargo build --release --locked
```

产物位置：

```text
target/release/zcode-tui
```

检查产物并安装到用户目录：

```sh
./target/release/zcode-tui --version
mkdir -p "$HOME/.local/bin"
install -m 755 target/release/zcode-tui "$HOME/.local/bin/zcode-tui"
```

上游 `install.sh` 面向 Linux，包含 GNU `install -D`、GNU `sed -i` 和 `/opt/ZCode` 路径处理。macOS 上应使用上面的手动安装命令；本机适配的 `zcode` wrapper 位于 `~/.local/bin/zcode`，它连接 `/Applications/ZCode.app/Contents/Resources/glm/zcode.cjs`。

## Linux x86_64 静态发布产物

正式 Release 使用 `x86_64-unknown-linux-musl`，产物是无需 Rust 工具链即可运行的静态 Linux 二进制。推荐在 Ubuntu 或项目的 GitHub Actions 中构建：

```sh
sudo apt-get update
sudo apt-get install -y musl-tools
rustup target add x86_64-unknown-linux-musl

cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --locked
cargo build --release --locked --target x86_64-unknown-linux-musl
```

原始产物位置：

```text
target/x86_64-unknown-linux-musl/release/zcode-tui
```

按上游 Release 的文件名和校验文件打包：

```sh
BIN=zcode-tui-x86_64-unknown-linux-musl
cp target/x86_64-unknown-linux-musl/release/zcode-tui "$BIN"
sha256sum "$BIN" install.sh > SHA256SUMS
```

最终需要发布三个文件：

```text
zcode-tui-x86_64-unknown-linux-musl
SHA256SUMS
install.sh
```

验证校验和：

```sh
sha256sum --check SHA256SUMS
```

## 在 macOS 上通过 Docker 打 Linux 包

macOS 不能直接使用 Linux musl linker。已安装 Docker 时，可以从仓库根目录运行：

```sh
docker run --rm \
  -v "$PWD:/work" \
  -w /work \
  rust:1.93-bookworm \
  bash -lc '
    apt-get update &&
    apt-get install -y musl-tools &&
    rustup target add x86_64-unknown-linux-musl &&
    cargo test --locked &&
    cargo build --release --locked --target x86_64-unknown-linux-musl
  '
```

构建完成后，在 macOS 宿主机生成发布文件与校验和：

```sh
BIN=zcode-tui-x86_64-unknown-linux-musl
cp target/x86_64-unknown-linux-musl/release/zcode-tui "$BIN"
shasum -a 256 "$BIN" install.sh > SHA256SUMS
```

## GitHub Tag 自动发布

`.github/workflows/release.yml` 会在推送 `v*` tag 时执行质量检查、Linux musl 构建、校验和生成和 GitHub Release 创建。发布前应确保 `Cargo.toml` 版本、`CHANGELOG.md` 版本标题和 tag 一致：

```sh
git tag vX.Y.Z
git push origin vX.Y.Z
```

Release notes 会从 `CHANGELOG.md` 中对应的 `## [X.Y.Z]` 段落提取。
