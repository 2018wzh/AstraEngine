# AstraEMU 独立 Family 与 Host 契约

AstraEMU 采用独立 Host 与 in-process family plugin。旧 `LegacyRuntimeProvider`、RuntimeWorld、Family ABI v9、Extension ABI、Host VFS、通用 Hook 和统一 save/package 不属于新接口，不提供兼容实现。设计决策和交付范围见 [独立 Host 重构](../migrations/astraemu-independent-host.md)。

## 描述与生命周期

Family descriptor 声明稳定 family/plugin identity、独立 ABI identity、格式与 capability。静态与动态插件使用相同接口；动态库在所有 session、回调和借用释放之后才能卸载。未知 ABI、缺少必要能力和不合法 descriptor 必须在打开游戏之前失败。

Host 调用 probe 后显式选择 Family；多项命中不依赖注册顺序。打开时传游戏位置，之后通过 elapsed advance、物理 input 和 window event 推进。Family 自行解释原生脚本并管理原生文件和存档，不接收 Host save root。整个进程只允许一个活动 session。焦点和前后台事件交给 Family 决定行为。

close 必须取消仍在进行的请求与阻塞音频写入，等待 Family worker 停止，再释放 session。不可恢复错误关闭游戏并回到资料库，保留可定位的诊断。错误不得跨 ABI unwind，也不得被替换为空帧或成功状态。

## 最终帧与音频

Family 借出 CPU 最终帧的只读 view。Host 验证尺寸、stride、format 和长度，在借用期间同步复制，然后释放借用；异步上传不得继续引用 Family 内存。GPU handle 不进入 ABI。

Family 完成解码和混音，PCM 流在 open 时固定 sample rate、channel count、i16/f32 format。独立 Family 音频 worker 向 Host 的有界、可取消队列阻塞写入。Host 完成设备采样率与格式转换；设备实时回调不调用 Family，不推动 VM。取消必须解除等待，格式或边界错误必须显式失败。

## 异步文字替换

这是单独、可选的 typed capability，不是通用 bytes Hook。Family 可提交正文和可选说话人/ruby，随后只挂起当前文字流程，继续音频和必要的 session 工作。完成后由 Family 使用原游戏字体检查字形、布局和绘制；Host 不提供翻译 overlay。

OpenAI-compatible service 默认 timeout 15 秒且可配置；失败/超时显示诊断并返回原文处置，不自动重试，不跳过段落。退出取消。上下文限最近 8 段且总计不超过 6000 字符；译文 cache 只在 session 内有界保存，新游戏、读档和配置改变时清空。正文、上下文和 secret 不进入日志或持久化缓存。

FVP 本轮必须声明此能力不可用，UI 禁用，不能伪造成功。端到端接入等待 Minori；本轮只测试独立服务与 ABI 错误边界。

## 滤镜与错误

Host 将复制后的最终帧上传到 Slint 共享 wgpu device，执行 HLSL 效果链，再供 Slint 合成。Magpie format 4 只接受明确支持的资源、pass、参数和内建函数；未知语法、能力缺失、编译失败均拒绝新配置，保持原有效配置。内置 shader 使用 MIT Anime4K 源码独立移植，兼容层不复制 Magpie GPL 实现。固定 DXC 生成 SPIR-V，再由 Naga/wgpu 校验；不采用 unsafe shader passthrough。

## 测试与迁移

新 ABI 替换旧接口与 loader，旧 Manager 数据明确重建，游戏原生存档保持由 Family 管理。ABI 需要覆盖无效 descriptor、frame bounds、PCM format、取消与关闭、插件卸载顺序及 translation capability。产品测试覆盖资料库、原生输入、音视频、系统页、存读档、退出冷启动和授权游戏任一结局；本轮不创建新的 evidence/report 留存体系。

Rust 类型是具体字段和 ABI layout 的真源：[Family API](../../Emulator/Source/FamilyApi/astra-emu-family-api/src/lib.rs)。状态见 [Stage 5](../status/stages/stage-5-astra-emu.md)。
