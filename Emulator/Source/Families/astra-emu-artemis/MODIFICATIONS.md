# Artemis 最小原生适配

上游为 [Alphaly2K/art3m1s-core](https://github.com/Alphaly2K/art3m1s-core)，固定基线 `0c06f37160961c9ff75d4937d5e6bb0500d0bef9`（0.4.0）。完整 fork 为 [2018wzh/art3m1s-core](https://github.com/2018wzh/art3m1s-core)，submodule 固定到本地单个适配提交 `13e55ffa98b8ea2fec4b294d210bd50567106a08`，尚未推送。

## 核心差异

- PFS 索引与查询统一路径分隔符，保留核心原有大小写语义；补充混合分隔符、完整读取和范围读取回归。
- 路径转换测试使用原生 Path 比较，删除 Unix 分隔符假设，生产路径转换不变。
- 根清单声明独立 workspace。可选 RFVP 桥的相邻目录依赖改为精确上游 revision `ec204312e123b4839cec8e69e6237fb0374e5518`，保留原有 feature；AstraEMU 不启用此桥，FVP 仍使用自身固定基线与 Family。
- Vulkan 选择仅接受独显、集显和虚拟 GPU，拒绝 CPU 与未知设备类型；原生 GPU 渲染及平台实现保持不变。
- 增加显式 Vulkan 生命周期测试，覆盖三次创建、帧读回、关闭及重开。

桌面保留上游 Lua 5.1 与 mlua send；未吸收旧分支为 VN 依赖冲突而引入的 Luau 替换、强制唤醒、修改游戏变量和模拟视频结束逻辑。

## 验证与未完成事项

Windows Vulkan 编译、GPU 生命周期测试、PFS 9 项测试和 Lua 5.1 的 227 项单元测试通过。Clippy 执行成功，上游警告未屏蔽。shaderc 原生构建需要足够短的本地构建目录，测试产物不共享到其他工作树。

Family v3、Manager 诊断与配置、PCM 桥及真实游戏验收尚未完成。核心 FFmpeg 视频实现已有解码与 GPU 呈现，但视频音频提取线程缺少 join，关闭时不能只更新代次就释放 session；接入前需完善生命周期或由上游修复。不能用延迟报告视频结束绕过该问题。

许可证与归属见 [第三方说明](THIRD_PARTY_NOTICES.md)。
