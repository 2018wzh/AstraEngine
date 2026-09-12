# Minori core

Minori 提供独立 `FamilyProvider`、`FamilySession` 和可选 `abi_stable` 动态模块。核心自己打开 PAZ、执行 SC VM、绘制 CPU 最终帧、混音并维护存档。`astra-text` 提供真实字体 shaping/排版/字形，`astra-media-core` 提供 CPU 图像合成；不创建 Engine World、package、registry、PlatformHost 或 Headless server。

## 启动和输入

`create_minori_provider()` 返回静态 provider；`dynamic-plugin-export` feature 导出独立 FamilyModule。descriptor 使用 Family API v2，接受 `profile_file`（默认 `minori.profile.json`）和 `entry_script`（默认 `test.sc`）两个 typed 启动配置。前者限游戏目录内安全相对路径；后者是 scr archive 内单个 `.sc` 文件名。`test.sc` 延续此前明确优先选择的观察入口，可按游戏修改。不会在入口失败后猜测另一个脚本。

已接入的渲染路径保留既有 1280×720 坐标与 Noto Sans JP 字体绑定。Host resize 只改变窗口，不改游戏内部布局。Session 按 60 Hz 逻辑处理有界 elapsed interval，显示纹理、消息、面板和已验证的 CrossFade2；字形结果与纹理有界缓存。Enter、Space 和鼠标主键推进消息；F5 保存，F9 读取。关闭和销毁都会取消文字请求、取消 PCM 写入并等待 audio worker 结束；同进程不能同时打开两个 Minori session。

正文替换是可选服务；禁用时直接显示原文。启用时只暂停当前消息推进，效果和音频继续。请求具有 session/generation identity，加载和关闭取消旧请求。失败、15 秒 timeout、错误 response 或无法排版的替换保留原文，不自动重试。正文和错误 payload 不写日志。

## 私有配置与存档

`mount_minori(game_root, profile_path)` 加载游戏目录内不超过 1 MiB 的 JSON。`MinoriProfile` 使用 `astra.emu.minori.profile.v1`，包含 `paz_version`、`index_size_xor` 和八个固定 role 的密钥设置。配置包含私有解密信息，不进 Git、日志或报告；读取失败不写文件。旧 Luau patch/YAML 不迁移，使用纯 Rust GARbro importer 重新导入 JSON。

PAZ 的边界/重叠校验、分卷读取、Blowfish/RC4、XOR、解压和源文件变化校验保留。`PazManifest` 是核心本地索引，不是 Host VFS 或产品 package。导入/读取不运行 managed helper、BinaryFormatter 或 executable patch。

核心自有 slot 位于游戏目录 `.astra-minori/saves/slot-000.asav`。容器 `AMINSV01` 包含长度、SHA-256 与 postcard snapshot，保存 VM、当前文本、等待进度以及 sound resource/播放位置/volume/pan/repeat。它绑定同一 archive/profile identity；加载重建脚本、场景和声音后提交。临时文件 flush 后原子替换；损坏、异版本或外部格式文件拒绝覆盖。此格式不宣称兼容原版存档。加载恢复声音的当前参数与位置，不保留尚未完成的 Kira fade tween。

## 音频边界

Symphonia 在核心 audio worker 内完整解码，Kira 执行 resampling、混音、pan、loop 和 fade，输出固定 48 kHz stereo F32 PCM 到 Host 有界队列。每个资源输入最多 64 MiB，总 decoded frame budget 为 32 Mi frames（256 MiB），最多 64 个 stream；超限明确失败。当前采用完整资源解码，尚不是长媒体流式 decode。Snapshot/restore 走有界 worker 命令，close 先取消阻塞写入再 join，不使用 detached decoder thread。

## 验证与剩余边界

```bash
cargo test --manifest-path Emulator/Cargo.toml -p astra-emu-minori --all-features
cargo clippy --manifest-path Emulator/Cargo.toml -p astra-emu-minori --all-targets --all-features -- -D warnings
cargo build --manifest-path Emulator/Cargo.toml -p astra-emu-minori --features dynamic-plugin-export
```

45 个普通测试覆盖 archive、图像、parser/VM、native codec、profile 边界和真实日文字形合成。公开最小字节格式 fixture 贯通 encrypted PAZ、SC、PNG、完整 PCM decode、Kira stereo pan、消息物理输入、atomic save/load 相同帧、translation cancel/load 和 close/reopen；单独验证阻塞 audio sink 退出、decode budget 与非挂起 VM 循环边界。Fixture audio sink 不是真实音频设备，动态模块编译也不代表 Manager 内实际安装验收。

现有未验证 stand positioning、movie、choice、系统页相关 opcode 仍明确失败，不能把这一可运行子集说成完整 Minori 兼容。ANI/SQZ decoder 有独立真实格式 fixture，尚未接入 session 动画播放。尚无授权完整游戏、真实设备音视频、原版 save compatibility、冷启动/完整结局、Android 静态注册或跨平台验收。
