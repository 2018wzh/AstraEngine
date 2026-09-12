# Minori core

Minori 的 PAZ 读取、SC parser/VM、ANI/SQZ 解码和文字绘制使用核心自有实现与独立 SDK。`astra-text` 提供真实字体 shaping/排版/字形，`astra-media-core` 提供 CPU 图像合成；不创建 Engine World、package、registry 或 Headless server。

本阶段先恢复可独立构建的核心库。旧 LegacyRuntimeProvider、Extension hook、Host VFS、trusted Luau patch 和旧动态导出已删除；独立 FamilyProvider/Session、音频与原生文件存档的接入尚未完成，不把库测试算作游戏播放验收。

## 私有配置

`mount_minori(game_root, profile_path)` 加载游戏目录内不超过 1 MiB 的 JSON。`MinoriProfile` 使用 `astra.emu.minori.profile.v1`，包含 `paz_version`、`index_size_xor` 和八个固定 role 的密钥设置。建议文件名 `minori.profile.json`。配置包含私有解密信息，不进 Git、日志或报告；读取失败不写文件。旧 Luau patch/YAML 不迁移，需通过纯 Rust GARbro importer 重新导入；CLI 接入此格式正在实施。

PAZ 的边界/重叠校验、分卷读取、Blowfish/RC4、XOR、解压和源文件变化校验保留。`PazManifest` 为核心本地索引，不是 Host VFS 或产品 package。图像/VM 的现有错误语义与公开最小 fixture 保留。

## 验证

```bash
cargo test --manifest-path Emulator/Cargo.toml -p astra-emu-minori
cargo clippy --manifest-path Emulator/Cargo.toml -p astra-emu-minori --all-targets -- -D warnings
```

40 个普通测试覆盖 archive、图像、parser/VM、原生状态 codec、私有 profile 边界及真实日文字形合成。尚无授权完整游戏、真实音频设备、冷启动/完整结局或跨平台验收。
