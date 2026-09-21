# RFVP Astra Family fork record

`../../../ThirdParty/rfvp/` is a complete Git submodule fork of the `0.6.0` upstream commit
`304e773387a9920c9db091ec1fd937c717aea949` from
[`xmoezzz/rfvp`](https://github.com/xmoezzz/rfvp). The hosted adaptation used by AstraEngine is published in the
[`2018wzh/rfvp` fork](https://github.com/2018wzh/rfvp/tree/codex/local-product-adaptation),
branch `codex/local-product-adaptation`, at immutable revision
`73d3b4413c95a3923cce695d98c3d9bf5b08ccf0`. It is the single adaptation commit
directly on that upstream baseline. The parent repository pins its exact gitlink,
and fetching the configured fork branch reproduces it. The covered source and the
complete MPL-2.0 text are in `../../../ThirdParty/rfvp/crates/rfvp/` and
`../../../ThirdParty/rfvp/LICENSE`.

The fork retains upstream history, workspace members, tools, licenses and platform
crates.
The Family depends on `crates/rfvp` with `hosted-gpu`; native video, bitmap and
Anzu feature dependencies retain their upstream definitions.

The Family configures four active BGM voices, matching RFVP 0.6.0's native
`BgmPlayer` slots instead of the hosted mixer's generic two-voice default.
Paused and fading voices remain charged against capacity; playback is never
made to succeed by dropping a voice or completing its fade early. SE and total
limits remain unchanged. This configuration change stays outside the fork.

The 0.6.0 update incorporates upstream text-wait completion, InputFlash,
dissolve-wait and native global-save fixes. Hosted sessions own script globals per session and never use the process-global
`GLOBAL`. Global/system persistence shares the upstream `GlobalSaveDataV1` type,
bincode codec and RFVG footer. Boot reads `save/rfvp_global.bin` before the VM
runs; explicit Family close writes it through the existing atomic filesystem
adapter. Missing files start a new state; malformed files, mismatched global
counts and I/O failures propagate without overwriting the existing save.
The previous duplicate hosted global capture implementation has been removed.
The unused hosted snapshot/restore and canonical state hash APIs were deleted.
Sixteen previously modified source files now match upstream exactly.
Input, time, timer and motion serialization additions were removed; native slot
snapshots now use the upstream graph and motion layout and load behavior.
The Arc texture ownership adapter remains for shared host rendering resources.

The local changes to the fork are limited to these areas:

- The hosted mixer exposes a read-only `voice_count(kind)` diagnostic, including
  paused and fading-out voices still charged against capacity. The Family logs
  this count separately from `is_playing` and the configured limits. Admission,
  mixing, fade timing and capacity limits are unchanged.
- `crates/rfvp/Cargo.toml` adds hosted and hosted-gpu features plus the SHA-256
  dependency used by native persistence. Upstream default features, sibling
  dependencies and library targets remain intact. Astra loads only the separate
  FVP Family dynamic plugin.
- `src/host_api/audio.rs`, `src/hosted.rs`,
  `src/audio_player/{bgm_player_host,se_player_host}.rs`, and
  `src/no_std_core.rs` add a typed `is_playing` query and synchronize host
  mixer completion back into RFVP's logical player state. This lets a
  non-looping voice reach EOF without leaving a stale wait predicate.
- `src/subsystem/resources/videoplayer_host.rs` and `src/no_std_core.rs` add a
  typed host stop request and a completion path that clears a host-owned movie
  without emitting a second stop operation.
- `src/soft_render/{framebuffer,renderer}.rs` remove the private
  `astra-byte-source` surface wrapper. Hosted rendering now exchanges a plain
  CPU `Vec<u8>` with the FVP adapter; the adapter performs the final
  Host-owned frame handoff.
- `src/subsystem/resources/text_manager.rs`, `src/font.rs`, and
  `src/no_std_core.rs` keep the four original system-font slots and accept
  host-owned bytes plus a TTC face index. No font is embedded or substituted;
  the Astra adapter resolves exact installed family names with `fontdb`, and
  missing optional faces fail only when selected for rendering.
- `src/rendering/gpu_prim.rs` and `src/wasm_entry.rs` match
  the pinned upstream files exactly; formatting-only fork changes were removed.
- `src/subsystem/save_state.rs` shares one capture implementation and the
  upstream region-specific global operations. User-save capture still requires
  the prepared VM snapshot and cannot silently substitute an empty VM.

- `src/no_std_core.rs` preserves native deletion of empty save slots: missing
  files are already deleted, while other filesystem failures propagate. A
  static operation label identifies failing runtime phases without exposing
  game text or filesystem paths.
- Hosted saves prepare the VM snapshot and thumbnail after every coroutine has
  yielded in the frame containing `SaveCreate`, matching the native frame
  boundary, and write that prepared buffer at `SaveWrite`.
  The codec uses the native RFVP calendar/NLS/thumbnail prefix and RFVS state
  footer in `save/rfvp_sNNN.bin`. Invalid headers and missing state fail;
  the earlier RFV9 and text-only readers have been removed. Slot caches are
  updated only after the file write succeeds. VM scheduling waits for host
  capture before another tick can mutate the prepared frame.
  Native slot loading sets each coroutine's return register to `true`, allowing
  the script to rebuild transient state after its saved yield. Queued slot
  refresh, copy, delete, write and load operations finish before scripts resume.
- `src/vm_runner.rs`, `src/subsystem/resources/text_manager.rs`, and
  `src/subsystem/resources/thread_wrapper.rs` cancel text-reveal waiters when
  their coroutine exits or restarts. A delayed completion only releases a
  text wait; it cannot restart an exited coroutine or release another wait.
  Hosted VM failures log the coroutine, instruction address and opcode without
  including script text or arbitrary error payloads.

The AstraEngine `astra-emu-fvp/src/` files are the separate family adapter.
They keep RFVP responsible for game state and use `NativeFileSystem` for
relative game-file access, native GPU rendering with final-frame readback, the bounded
host audio worker for mixed PCM, and the session-owned WMV playback path for
movie duration, frame timing, and completion. The adapter does not add a
second scene renderer, translation cache, save format, or runtime snapshot
format.

`Docs/emu/fvp/rfvp-fork-audit.md` records the historical v9 audit. It is not
a description of the current independent-host source or its release status.

## Family API v7 启动配置

Family adapter 声明 `script_encoding` enum（shift_jis/gbk/utf8），经 v7 typed schema 验证后传给已有 `HostedBootConfig.nls`。默认仍是 ShiftJIS；没有修改 RFVP VM、编解码器或 GlobalSaveDataV1/RFVG。翻译 capability 仍未声明。

## 全局存档失败诊断

Family `NativeFileSystem::copy` 在原子替换前使用可写临时文件句柄同步数据，并先关闭句柄再替换。此前只读句柄使 Windows 同步调用失败，真实自动播放中的存档复制因此终止会话。本机成功创建/覆盖复制回归先复现失败，修复后通过；源文件与阻塞目标保护继续保留。复制失败诊断仅记录操作阶段、IO 错误类别和系统错误码，不输出路径或存档内容。此改动仅在 Family 适配层，不改变上游核心 gitlink 或原生存档格式。

Hosted 读取分别记录 RFVG 解码失败、footer 缺失与状态不匹配。状态不匹配日志只包含版本、变量区及已读位图计数，不输出文件位置或游戏内容；失败仍在应用状态前返回，不更改文件。

## 原生 GPU 嵌入

`hosted-gpu` 开放已有 `GpuPrimRenderer`、纹理、Sprite/Fill 管线和 `RenderTarget`，保留 RFVP 的 wgpu 0.19 版本与平台后端。新增离屏入口从 Core 的 MotionManager 绘制，不经 Family 重建场景或复制 shader。Hosted 图像缓冲只补充现有渲染器需要的 `dimensions`/`from_pixel` 方法。

最终帧回读使用原有 RenderTarget 的行对齐与映射实现；新增有界、可返回错误的映射入口供 Family 使用。Family 拒绝软件 adapter，允许 Windows Sandbox 的虚拟 GPU。

原生 `rfvp_render/mask.rs` 补齐旧画面捕获、遮罩阈值与两段透明度演出，Hosted 与 `app.rs` 共用同一 GPU 管线。遮罩读取 NVSG 的 alpha 通道，演出进度沿用原游戏整数计算；缺失或尺寸不匹配时返回错误。缓存代数只用于渲染失效，不改变原生存档格式。中途恢复仍需验证。

独立 `gpu-render` 构建修正 Hosted 共享图像及字体结果的 feature 边界，不改变原生字体选择策略。离屏与窗口后端均保留各自的逻辑尺寸和渲染尺寸。

## 动态模块重复启动

Family 的 session map 改为持有 `Box<FvpSession>`。原先第二次启动在 `BTreeMap` 的节点插入路径上为整个 session 生成大型栈临时值，导致 Windows GUI 线程栈溢出；现在节点只移动堆指针。核心状态、关闭顺序与存档 codec 均保持原样。回归通过动态模块入口在 1 MiB 栈中连续启动、关闭三次。

## 图像恢复

`GraphBuff` 重新加载纹理后恢复存档中的颜色与显示位置，避免加载器的初始化覆盖保存值。调色会修改像素，因此调色后的图像使用既有 `RawRgba` 分支保存；从源文件重新加载无法还原这些累积修改。没有新增存档结构或渲染路径。GPU 回归覆盖原地及新会话恢复，完整游戏读档仍待验证。

## Manager diagnostics

The Family adapter installs the shared optional Family API v7 diagnostic bridge before descriptor/probe/open. Existing core tracing and log events reach the Manager sink without adding a core logger or changing native rendering/platform behavior. The bridge forwards bounded text and Debug values without content redaction; Manager-side field and size limits remain in force.

Family audio errors retain the failed operation (load, play, mix, stream submission or parameter update), and command failures emit `astra.emu.fvp.audio.operation_failed` with only operation and diagnostic code. Playback-state synchronization and video completion have distinct error contexts. Mixer limits and core behavior are unchanged; these diagnostics narrow the unresolved capacity failure seen during the Sandbox playthrough.

Hosted save capture and restore emit `rfvp.save.text_state` at DEBUG with numeric font, color, outline and reveal state for loaded text slots. This diagnoses visual restoration differences without logging the text or retaining pixel dumps. It does not change the save codec or restore behavior.

## Timed motion restoration

RFVS snapshot version 2 preserves the existing alpha, move, rotation, scale, depth, V3D, sprite, snow and lip containers, including elapsed time and allocation state. Restore resumes these native containers instead of discarding them. Mask dissolve types 4–6 are restored explicitly. RFVG global persistence is unchanged. Version 1 RFVS snapshots lack the required motion state and are rejected without rewriting the source file; tests must use new slots. The snapshot payload remains bounded and uses the existing bincode codec.

Motion restore rejects out-of-range or duplicate image slots and unknown dissolve types before changing the current scene. Invalid slots are no longer skipped or applied in input order. This validation does not introduce another persistence format or rendering path.
