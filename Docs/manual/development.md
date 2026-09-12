# 开发与测试

在仓库根目录运行 `cargo xtask`。Engine/VN/Player/共享库由根 workspace 管理，EMU 使用 `Emulator/Cargo.toml` 和独立 lockfile/target；Editor 将在实际 GPUI 实现接入时加入独立 workspace。共享库通过路径依赖复用，不复制源文件或产物。

```bash
cargo xtask docs
cargo xtask check --workspace engine
cargo xtask check --workspace emu
cargo xtask test --workspace engine -p astra-core
cargo test --manifest-path Emulator/Cargo.toml -p astra-emu-family-api
```

`check` 执行链接检查、格式、clippy、产品构建和完整测试；`fmt` 只处理所选 workspace 的成员，不格式化第三方路径依赖。`test -p` 是普通定向测试，不启动或预构建 Headless。完整产品测试先构建所选产品的程序，供真正的 CLI/宿主测试使用。

并行开发每个实例使用独占 worktree 和 target；不要设置指向其他 worktree 的 CARGO_TARGET_DIR。构建/测试失败必须修复或准确记录根因。

文档检查仅检查链接、控制字符与私有绝对路径，允许 TODO 和未完成状态，不要求报告 schema 或指定 Stage 文字。普通 Rust 测试验证数据和行为，GPU/音频/设备测试按实际需要运行。开发 Agent 辅助真实产品验收，不生成具名审批体系。

Windows 完成长流程，Linux/macOS/Android 完成代表流程和平台特性；没有 GPU、音频设备或商业源时不得声称相关真实流程通过。
