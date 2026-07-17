# AstraEMU Manager 元数据与游玩状态

Manager 扫描授权目录时，只读取 discovery descriptor 声明的入口和 marker。扫描不会执行脚本，也不会把路径、文件 hash、脚本文本或素材上传。family probe 与作品匹配是两条独立链路，外部元数据不能修改 family binding。

首次使用 VNDB 或 Bangumi 前，打开 **Metadata**，分别启用 provider。Bangumi 搜索不强制 token；关联收藏和同步游玩状态需要 access token。token 写入平台 secret store，Library 只保存 `astraemu.metadata.bangumi.token` reference。关闭 provider 后，本地扫描、probe、已保存 snapshot 和 review queue 都保留。

选中游戏后点击 **Refresh**。未关联作品时，Manager 以本地标题搜索并把所有名称匹配放入 Review queue。查看标题、别名、日期、开发者和逐项 evidence 后，逐条 Accept 或 Reject。系统不提供模糊匹配批量接受。也可以输入 VNDB `v123` 或 Bangumi subject ID；Manager 先向对应 provider 校验，成功后才以 `user-verified-id` provenance 建立关系。

**Unlink** 只解除当前作品与 provider 的关系，不删除安装。删除安装后，work 仍可保留 snapshot 和用户保留标志；清理 orphan work 必须走单独的维护动作。

安全封面默认启用。敏感封面开关按 provider 保存；开关关闭时，带 VNDB sexual/violence 或 Bangumi `nsfw` 标记的图片不会下载，也不会写入缓存。下载过程拒绝重定向和非 HTTPS 域名，并限制 MIME、响应大小、像素尺寸和解码预算。

作品已经关联 Bangumi 后，可设置 `wish`、`doing`、`collect`、`on_hold` 或 `dropped`，填写 1 到 10 分和私密备注，再点击 **Sync play status**。Manager 先保存本地待同步状态，再把请求交给后台 worker。成功时间和失败 diagnostic 都会持久化；429、401、超时或 schema mismatch 不会触发无限重试，需要用户显式重试。

VNDB 默认只适用于 development/non-commercial profile。商业构建必须在 release manifest 中提供明确的 VNDB commercial license ID；缺失时 `emu.metadata_license` 必须阻断发布。当前正式 release gate 仍在 `IN_PROGRESS`，不能把 provider unit test 当作商业发布许可证明。
