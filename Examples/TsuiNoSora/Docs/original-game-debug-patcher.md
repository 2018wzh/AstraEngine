# 1999 原版调试菜单补丁器

这套工具面向已验证的 1999 Windows 版安装。它不会修改原安装目录，而是在 `--output` 生成完整副本。标题第三个按钮随后进入原版 frame 75 调试菜单；左上 64×64 隐藏入口不受影响。

补丁器不使用固定文件偏移。ProjectorRays 0.2.0 先把副本中的 `DATA/MENU.dxr` 还原成可编辑 Director 容器，内置 reader 再沿 `RIFX/imap/mmap/CAS*/CASt` 资源图定位行为绑定。退出按钮必须唯一对应 `CASt-88`、member 42、`scriptId=9`，调试菜单必须唯一对应 `CASt-381`、member 63、`scriptId=44`。任一旧值、类型、资源数量或边界不符，补丁都会停止。工具只改前者的 4 字节 script ID，不复制 Lingo 正文。

窗口模式分两层处理。与 projector 同名的 INI 负责关闭全屏、禁止切换色深和缩放，并让 800×600 stage 居中。Director 7 仍会留下 1 像素 outer frame，游戏画面没有填满整个外窗。补丁目录因此附带 `TsuiNoSoraWindowed.exe`：它在启动前复验成品 hash，随后启动与 `SETUP.exe` 字节相同的 `TsuiNoSoraProjector.exe`，避免现代 Windows 按文件名把旧游戏误判为安装程序。launcher 只接受该进程创建的可见 800×600 client window，删除 outer frame 后保持 800×600 outer size，再按显示器 work area 重新居中。找不到目标窗口或 Win32 调用失败时，launcher 会终止该次启动并返回 diagnostic。

乱码在启动边界解决。launcher 使用未修改的 Locale Emulator Core 2.5.0.1 创建 32 位 projector 进程，参数固定为 CP932、Japanese LCID、Shift-JIS charset 与东京时区。补丁不依赖已安装的 Locale Emulator，不读取用户 profile，也不改系统区域。core DLL、版本、revision、LGPL-3.0/GPL-3.0 许可证和 hash 都由发布包与成品 manifest 锁定；加载或注入失败时不会退回当前系统 code page。

`patch-manifest.json` 只保存相对路径、文件 hash、补丁版本、ProjectorRays 身份、Director resource/member/script ID 和窗口策略。它不保存原作脚本、素材 payload 或本机路径。发布目录中的 ProjectorRays 固定为 0.2.0，并附带上游 MPL-2.0 许可证；helper 和许可证都按固定 SHA-256 校验。

命令和发布包结构见 [补丁器 README](../Tools/original-patcher/README.md)。商业安装源、补丁后游戏和运行截图仍属于 ignored 私有验收证据。
