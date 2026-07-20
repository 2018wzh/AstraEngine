# Classic 第一线路私密 RC

## 发布边界

本切片只面向固定小群的 private research preview。交付 profile 固定为 Classic 和日文；加密 package 保留当前完整转换内容，但只对第一线路 `Y`、Title 与 Classic 系统页作可玩性承诺，其他路线属于未验收 research 内容。Modern 和 Director gameplay direct-read 不属于本次交付。2026 年 7 月 20 日 00:00 的目标时间已经过去；加密、Headless E2、Y 范围 13/13 视觉比较或人工签署任一项未通过，整包继续延期。

Windows x64 bundle 仍是候选 artifact，但本轮不把 Windows E3 作为发布门禁。bundle 只做结构、身份和 source unlock 校验；真实窗口输入、音频、存档恢复与同 run identity 的 E3 保持 `IN_PROGRESS`，不能用 Headless E2 代替或写成已通过。

原版目录只参与版本校验和内存密钥派生。它不能证明购买关系，也不能替代权利人许可。Artifact 不进入 GitHub Release、公共 CI artifact、README 下载链接或公开 Issue。

## 原版截图契约

新增原图接受两种严格格式。原生 800×600 RGBA PNG 直接作为归一化结果，不裁剪、不缩放，也不改变色彩空间；旧的 802×602 RGBA 捕获必须具有四边完全一致的 1 px 边框，处理器只裁剪 `(1,1)-(801,601)`。其他尺寸、颜色模式、边框或稳定帧不一致都会立即失败。公开仓库只保留 [脱敏截图凭据](classic-first-route-recapture-manifest.json)，原图和五联图仍位于 ignored 私有工作区。

已确认的第一线路顺序是：四张累计 Opening bitmap、视点 bitmap、日期算式 bitmap，然后进入三句连续黑场对白。UI005 绑定 `Y` frame 26；旧的 `K` frame 138 映射已删除。UI009 绑定 `Y` frame 32；两张选择画面逐字节一致，选择后的下一帧证明 shade 只在选择成功后清除。

## Director 入口层恢复

Director movie 入口不等同于“空舞台”。`startMovie → tinit` 会恢复 Score 初始可见 channel。Y 的 channel 3 可以先更新房间背景，但 channel 5 的 800×600 black sprite 仍在更高层，因此开场对白继续显示黑色舞台。后续 character 命令替换 channel 5 后，channel 3 才可见。

转换器现按每个 movie 的入口 Score snapshot 生成 typed stage layer 初始化：先清理不属于新 snapshot 的旧 layer，再按 channel 顺序提交初始可见 sprite。`back`、character、event 与 shade 只修改各自的权威 layer，不能顺带清除其他 channel。该规则应用于全部 Director movie，不含作品或截图节点特判。

## Source-bound package

公共 contract 新增 `astra.source_unlock_policy.v1` 与 `astra.crypto.source_fingerprint.v1`。支持版本由 [Classic source profile](classic-source-profile.json) 固定：

- 平台 provider 返回不透明的用户授权目录对象，只允许安全相对路径的 bounded stat/read；
- manifest 同时校验 profile、文件集合、长度和 SHA-256；
- key material 从已验证的原版文件字节计算，不直接使用公开 SHA-256 或 Merkle root；
- section key 经 HKDF-SHA-256 派生，商业 section 使用 AES-256-GCM-SIV 和 metadata AAD；
- key 与读入文件字节使用后清零；认证失败、provider 不匹配、AAD 或 ciphertext 篡改均阻断；
- bootstrap policy 保持明文，声明的 protected section 必须全部存在且全部加密。

package build、Windows bundle 与 Player bootstrap 已接入同一主路径。CLI 只有同时取得 policy、source profile 与授权 source root 才能 Cook source-locked package；bundle 会再次校验原版，密文原样进入 bundle，并写入固定相对路径的公开 source profile。Player 启动时由用户重新选择原版目录，目录只以不透明 handle 进入 bounded stat/read。受保护 scenario 不会被 bundle 解密成外部文件。

当前证据已经覆盖 package、平台 capability、CLI 与 Player 的 contract/E2：正确 source roundtrip、修改 source、越界路径、ciphertext/AAD tamper、缺失 protected section、明文扫描和错误 provider 都有定向测试。Cook 与密文 package 保留完整 37 条转换路线；发布报告必须把 `Y` 标为唯一 guaranteed route，并把其余路线明确标为 present-but-unvalidated，不能用“包内存在”推导可玩性。

构建接口要求三项 source-lock 输入同时出现，缺一即由 CLI 参数层阻断：

```sh
cargo run -p astra-cli -- package build <cooked> --out <package> \
  --source-unlock-policy <policy.json> \
  --source-profile <source-profile.json> \
  --source-root <authorized-original-root>

cargo run -p astra-cli -- package bundle <package> --out <bundle> \
  --target <target> --profile classic --platform windows \
  --windows-player <player> --crash-reporter <crash-reporter> \
  --source-profile <source-profile.json> \
  --source-root <authorized-original-root>
```

source-lock build 会强制保护全部 cooked asset、extra section、cook summary、VFS/catalog、media manifest 与 scenario refs。policy 少列任一 product payload section 都会失败，不能由调用者选择性留下明文。

## 当前门禁

截图与同节点身份已经更新，UI005/UI009 的重取证缺口已关闭。v13 预检 package identity 为 `sha256:d7c81ec4f4494820d8410b2f88927dad31dbb5eb0ade3c03ec04090c1c796ea0`。同一预检 build/package 身份下有两份互不替代的 Headless E2：视觉矩阵通过 43 个 checkpoint；Y 路线见证用 445 条序列化物理输入、27 次选择，从 Title 进入 Y，并抵达 K movie 的第一个权威 wait。该见证不调用直接 command，也不写 Runtime 状态。最终 build identity 必须在截图凭据和文档冻结后重新生成，不能沿用预检 identity。

私有 reference 已收到 UI001、UI002、UI003、UI005 至 UI014 的全部画面。30 张输入均通过严格格式检查，12 组 A/B 契约全部逐字节稳定；UI002 没有动态复帧要求，其稳定性由 Score bitmap 的 resource hash 闭包证明。像素预检对 13 项均得到通过结果，模型也已查看全部五联图；UI010 至 UI014 的系统窗外框和语义矩形偏移均为 0 px，UI009 的菱形列偏移为 1 px。处理器已原子生成权威私有 manifest，并同步公开脱敏 recapture manifest、reference hash 与 node validation。UI004 与 UI015 不进入本轮 RC 的 13 项视觉门禁。

当前硬件 renderer、43 个视觉 checkpoint、完整 Y 路线、37 路线内容存在、Windows candidate bundle、source profile binding、商业明文扫描和私有路径扫描均已通过预检。release gate 会同时验证 13 项像素结果与截图稳定性凭据；截图来源门禁已经闭合，下一步是在冻结后的最终 build identity 下重跑 Headless、Y 路线和比较，再由用户完成 formal human signoff。Windows E3 本轮显式延期，不属于 RC gate。提交级 Python、补丁器、文档、格式、Clippy 与 Headless build 已通过；`cargo test --workspace` 在执行测试前被 MSVC `LNK1189` 对象库数量上限阻断，不能写成全量测试通过。最终签署前，本切片保持 `IN_PROGRESS`，不得写成已发布、已授权或已解决版权问题。

## 私密交付

`build_private_rc_delivery.py` 是最终交付的唯一组装入口。它只接受状态为 `passed` 的 private RC release report；任何视觉项、source binding、安全扫描或人工签署仍为 blocking 时，工具在复制文件前退出。组装过程重新校验 bundle manifest 中每个文件的大小和 SHA-256，拒绝链接、重复路径、不安全相对路径和身份漂移，并通过同目录 staging 原子提交结果。

交付目录会增加 `PRIVATE_RESEARCH_NOTICE.txt` 和 `private-rc-delivery-manifest.json`。manifest 记录 build/package/evidence/signoff hash、生成时间、七天失效时间、文件清单和完整性 hash，不记录本机路径、原版文本或 key material。声明明确限定固定小群、禁止转发和公开索引，链接只能整体撤销。Windows E3 在本轮仍标记为 `deferred`，不会被 Headless E2 改写成已完成。

```sh
python Tools/TsuiNoSora/build_private_rc_delivery.py \
  --bundle <verified-windows-bundle> \
  --release-gate <passed-private-rc-gate.json> \
  --out <private-delivery-directory> \
  --generated-at <utc-rfc3339> \
  --retention-days 7
```
