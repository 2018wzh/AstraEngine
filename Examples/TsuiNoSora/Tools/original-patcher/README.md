# 99 版原版调试菜单补丁器

本工具只接受已锁定指纹的 1999 Windows 版。它读取原安装目录，在用户指定的 `--output` 生成一份完整、可独立运行的游戏，不覆盖或重命名源文件。

```bash
TsuiNoSoraOriginalPatcher inspect --source /path/to/original-game
TsuiNoSoraOriginalPatcher apply --source /path/to/original-game --output /path/to/patched-game
TsuiNoSoraOriginalPatcher verify --output /path/to/patched-game
```

`apply` 只在同盘私有临时目录中工作。它把 `DATA/MENU.dxr` 解包后原名写回完整副本，将标题第三个按钮从退出脚本绑定改到原版调试菜单脚本绑定，并生成窗口策略。所有检查通过后，临时目录才会原子提交为 `--output`。输出已存在、路径重叠、版本指纹不符、ProjectorRays 身份不符或成品复验失败都会阻断。

Director 7 在窗口模式下仍会留下 1 像素 outer frame。生成目录因此包含 `TsuiNoSoraWindowed.exe`。应从该入口启动游戏；它先复验整个补丁 manifest，再启动与原 `SETUP.exe` 字节相同的 `TsuiNoSoraProjector.exe`，删除 outer frame，使 800×600 client area 填满整个居中、不可缩放的无边框窗口。保留原文件名的 `SETUP.exe` 仍按字节存在，额外文件名也避免现代 Windows 把旧游戏误判为安装程序并要求提升权限。

launcher 还会通过固定版本的 Locale Emulator Core 创建日文区域进程，显式设置 ANSI/OEM code page 932、LCID `0x0411`、Shift-JIS charset 128 和东京时区。它不读取机器上的全局 Locale Emulator profile，也不修改系统区域、注册表或原安装目录。core DLL、LGPL-3.0/GPL-3.0 许可证及其 SHA-256 都进入补丁 manifest；缺文件、架构不符或加载失败会阻断启动。由于 core 只支持 32 位进程，正式补丁器固定使用 `i686-pc-windows-msvc` 构建。

发布包通过 `package_release.py` 组装，包含 32 位补丁器、`projectorrays-0.2.0.exe`、Locale Emulator Core、上游许可证和 `bundle-manifest.json`。脚本会核验所有 helper、许可证和补丁器 hash，不下载依赖，也不接受已有输出目录。
