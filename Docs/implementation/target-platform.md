# Target And Platform Blueprint

Target 描述“构建和发布哪个产品入口”，Platform 描述该入口依赖的原生 host 能力。Target manifest 不包含 native handle、设备对象或平台 fallback 实现。

## Target Manifest

`project.yaml.targets` 是 Target 真源，Cook 后写入 `target.manifest` / `astra.target_manifest.v1`。发布 package 只能包含一个选定 Game target；Editor 与 Program target 不得混入 Game package。

每个 target 至少声明 id、kind、默认 profile、platforms 与 packaged。CLI 在 Cook、Package、Bundle 与 Validate 阶段持续校验同一 target/profile identity。

## Platform Profiles

`project.yaml.platform_profiles` 是强类型 `PlatformHostProfile` map。map key 必须等于 profile id；profile 的 target、package id 与 platform 必须和选定 Target 一致。Cook 后写入 `platform.profiles` / `astra.platform_profiles.v1`。Player 从 package 读取发布策略，不接受 CLI 覆盖 provider 或 fallback。

Windows release provider：`wgpu_hardware`、`wmf`、`wasapi`、`saved_games`。

Web release provider：`webgpu`、`webcodecs`、`webaudio`、`opfs`。本轮仅声明 Chrome，不提供 WebGL、IndexedDB、media element 或 software decode fallback。

## Capability And Conformance

`astra.platform_capability_report.v2` 记录 platform、target、profile id/hash、build fingerprint、SDK 状态，以及 renderer/decode/audio/save 的 declared、available、selected。普通 toolchain probe 不能把接口存在性或编译成功当作 available。

`astra.platform_host_conformance_report.v1` 记录 platform、target、profile hash、package hash、build fingerprint、session id、required check 和脱敏 evidence。Release Gate 要求 capability、conformance 与 Player automation 在 platform/profile/package/build/session identity 上连续匹配。

Linux、macOS、iOS、Android factory 当前固定返回 `PLATFORM_NOT_IMPLEMENTED`，不生成伪 renderer/audio/decode/save availability。

## CLI And Gate

```bash
astra target validate project.yaml --target nativevn-game --format json
astra cook project.yaml --profile desktop-release --target nativevn-game --out target/cooked
astra package build target/cooked --target nativevn-game --out target/nativevn.astrapkg
astra package validate target/nativevn.astrapkg \
  --profile desktop-release \
  --target nativevn-game \
  --platform-report target/platform-capability.json \
  --platform-conformance-report target/platform-conformance.json \
  --player-automation-report target/player-automation.json
```

正式报告只记录 schema、稳定 id、hash、provider、状态、计数和 diagnostic，不记录本地绝对路径、用户名、商业 payload、secret 或 native handle。普通 CI 负责 schema、负向门禁、单元测试与 wasm 编译；Windows/Chrome `DONE` 需要同一最终 commit 的真实验收。
