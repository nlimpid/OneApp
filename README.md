# OneApp

基于 Rust + GPUI 的桌面端 Hacker News 客户端。

## 功能

- 查看 Hacker News Top Stories
- 查看文章详情与评论树（支持折叠）
- 内置阅读模式打开原文链接（可跳转系统浏览器）

## 开发

### 环境要求

- Rust（stable，项目内已提供 `rust-toolchain.toml`）
- macOS：Xcode Command Line Tools（`xcode-select --install`）
- Linux（Ubuntu/Debian 示例）：
  - `sudo apt-get install -y pkg-config libx11-dev libxcb1-dev libxkbcommon-dev libwayland-dev libxrandr-dev libxi-dev libxcursor-dev libxinerama-dev libasound2-dev libudev-dev`
- Windows：Visual Studio 2022（MSVC）构建工具链

### 运行

```bash
cargo run
```

### 检查与格式化

```bash
cargo fmt
cargo clippy -- -D warnings
```

### macOS 打包（.app）

```bash
cargo install cargo-bundle
cargo bundle --release
open target/bundle/osx/OneApp.app
```

## License

MIT，见 `LICENSE`。
