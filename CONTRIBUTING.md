# Contributing

## 开发环境

- Rust（stable）
- macOS：`xcode-select --install`

## 本地运行

```bash
cargo run
```

## 代码风格

```bash
cargo fmt
cargo clippy -- -D warnings
```

## 提交 PR

- 尽量保持改动聚焦、可审阅（一个 PR 做一件事）。
- 说明动机与实现方式，并附上截图/录屏（UI 改动）。
- 确保 CI 通过（格式化、Clippy、构建/测试）。

