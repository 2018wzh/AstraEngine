# FVP 呈现与媒体

Family 复用 RFVP 原生资源、执行和绘制；Manager 接收最终 CPU lease 与有界 PCM。接口见 [Family API](../../../Emulator/Source/FamilyApi/astra-emu-family-api/API_CONTRACT.md)，核心差异见 [MODIFICATIONS](https://github.com/2018wzh/rfvp/blob/95d3ee23548cb32e035aaaaeab2d309fc5d3020e/crates/astra-emu-rfvp/MODIFICATIONS.md)。

## 图像与文字

`GraphLoad`、`GraphRGB`、`GaijiLoad` 修改原 graph/glyph 资源；`PrimSetSprt`、`PrimSetText`、`PrimSetTile` 及属性命令修改原 primitive 状态。HZC1/NVSG 解码留在原核心，格式及样本统计见[归档格式](archive-format.md)与[样本观察](game-observations.md)。

`TextBuff`、`TextPrint`、`TextFormat`、`TextFont`、`TextColor`、`TextSpeed` 等由原 text manager 处理，保持字体选择、已读状态、文字等待与绘制语义。系统字体由 Family 解析实际字体和 TTC face 后注入；当前 FVP 不声明文本替换能力。

原 `GpuPrimRenderer` 与 `RenderTarget` 负责图像、文字、转场和 mask。Family `SceneTracker` 只观察绘制元数据用于变化检测，不保留纹理 payload 或另行 rasterize。只有实际 GPU cache 确认 generation 已上传，才能授权 CPU 像素回收。

## 音频与影片

原音频命令通过 Family 有界 worker 进入原 SoftAudioMixer，混音结果交给 Host PCM sink。BGM 保持四个原生 slot，Movie 使用独立 stream ID；暂停/淡出仍占容量。控制处理与阻塞 PCM 写入分离，关闭取消 sink 并等待 worker。

影片使用原 RFVP Movie decoder、墙钟、合成与音轨路径；Family 的第二 WMV 播放器已删除。影片参数、结束、停止和 VM 等待由原实现决定，不在 Host 将游戏帧覆盖成独立视频帧，也不为接入改用另一平台 decoder。

## 帧与保存边界

变化帧提交原 GPU 并在完成后发布 CPU lease；静态帧返回 Unchanged。RFVP 回读复用 staging buffer，保留期限、取消和代次校验；CPU 输出分配成本仍纳入性能基线。

保存缩略图在原 SaveCreate 边界读取已呈现的 render target，使用原 Triangle 缩放。V1 槽与全局 RFVG 继续由核心编码；图像恢复和协程/保存时点修复按原生缺陷单独审查，不将其自动归为嵌入钩子。

输入沿原按键/指针入口进入 VM。Manager 负责窗口输出与逆映射，Family 不复制窗口 UI；原 no_std 输入提交缺口列为当前优先修复项。

实际验证、R01–R09 和下一步分别见[性能审计](../../status/performance-upstream-audit-2026-10-12.md)与[实施计划](../../status/implementation-plan.md)。
