# Target And Platform Blueprint

Target 描述“构建和发布哪个产品入口”，Platform 描述该入口依赖的原生 host 能力。Target manifest 不包含 native handle、设备对象或平台 fallback 实现。

## Target Manifest

`project.yaml.targets` 是 Target 真源，Cook 后写入 `target.manifest` / `astra.target_manifest.v1`。发布 package 只能包含一个选定 Game target；Editor 与 Program target 不得混入 Game package。

每个 target 至少声明 id、kind、默认 profile、platforms 与 packaged。CLI 在 Cook、Package、Bundle 与 Validate 阶段持续校验同一 target/profile identity。

## Platform Profiles

`project.yaml.platform_profiles` 是强类型 `PlatformHostProfile` v2 map。map key 必须等于 profile id；profile 的 target、package id、platform 与 verified package cache 限额必须和选定 Target 一致。Cook 后写入 `platform.profiles` / `astra.platform_profiles.v2`。Player 从 package 读取发布策略，只显式迁移 v1，不接受 CLI 覆盖 provider 或 fallback。

Migration 11 不把 Headless 加入 `PlatformId`、Target `platforms` 或 `platform_profiles`。planned `HostLaunchProfile` 只在 host 启动边界区分 `Platform(PlatformHostProfile)` 与 `Headless(HeadlessHostProfile)`；发布 profile 和 package section 继续只接受六个平台。`astra.headless_host_profile.v1` 属于测试配置，不能进入 Cook、Package、Bundle 或 Player。

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

真实产品平台验收在 Migration 11 完成后增加强制 Headless preflight：同一 build fingerprint、cooked package hash、input sequence hash、scenario、target 和 content identity 先通过 `astra.headless_run_report.v1` 与 `astra.headless_review.v1`，再由 `astra.headless_preflight_link.v1` 关联真实平台 run。缺 preflight 或 identity mismatch 时不得启动正式验收。
