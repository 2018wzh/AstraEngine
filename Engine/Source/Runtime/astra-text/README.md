# astra-text

直接从字体字节建立 font database，使用 cosmic-text 进行 shaping、fallback、排版和字形栅格化。实现从 astra-media 的文字模块迁出，Engine 通过 astra-media 使用同一套实现；Musica 可以直接依赖本 crate，不需要创建 RuntimeWorld、package、registry 或 Headless session。Musica 接入尚未完成。

## 使用与所有权

调用方持有 `CosmicTextLayoutProvider`，用 `new(context, fonts, config)` 传入字体。`PackagedFont` 保留既有名称，但它只持有调用方提供的字体字节、SHA-256、face index、license、coverage 和 target/profile 声明，不读取 package 或磁盘。`FontBindingContext` 的 target/profile 用来校验字体 eligibility；独立调用方同样需要显式提供匹配值。

`TextLayoutRequest` 指定正文、语言、方向、OpenType features、约束和有序字体链。`layout_shared` 返回缓存中的 `Arc<TextLayoutResult>`，包含实际 shaped glyph、bitmap、行与 cluster/source range。结果可直接交给自有绘制器；需要 Scene2D 命令时使用 `TextRenderResourceOwner` 管理 upload/draw/release。`shutdown` 返回资源释放命令，调用方负责提交到绘制器。

字体安装、替换与卸载保留 hash 校验、事务失败和缓存失效语义；空字库、缺失字体、非法 coverage、无效 fallback 和预算超限返回既有 `ASTRA_TEXT_*` diagnostic。字体 bytes 只通过显式 API 输入；本 crate 不增加文件/网络权限入口。库只发 tracing 事件，不初始化 sink；日志 category 为 `astra_text`。

## 依赖边界

直接依赖 `astra-core`（Hash256/Diagnostic 等值类型）、`astra-media-core`（GlyphBitmap、SceneCommand 和 MediaError）、`astra-worker-budget`（进程级有界 CPU 工作预算），以及 cosmic-text、serde、schemars、tracing、unicode-segmentation。media-core 还依赖 astra-byte-source，并包含 CPU renderer/filter executor，因此这一阶段仍有媒体 contract/CPU executor 的链接耦合；未把它声称为完全独立的最小字体算法库。

普通与测试依赖均不引入 astra-runtime、astra-package、astra-asset、astra-media 或 Headless server。现有 worker budget 仍使用全局 broker，并非新建 registry。未来是否拆分 media-core 由实际共享绘制消费者决定。

## Engine 迁移

`astra-media` re-export 这里拥有的文字类型，保持 `astra_media::TextLayoutRequest` 等类型身份。验证 package/VFS 并加载字体的代码仍归 astra-media：

- `CosmicTextLayoutProvider::from_package(...)` 改为 `astra_media::text_layout_from_package(...)`。
- `CosmicTextLayoutProvider::from_package_with_crypto(...)` 改为 `astra_media::text_layout_from_package_with_crypto(...)`。

package gate、Player 和测试调用方同步迁移；验证逻辑和返回类型不变。旧文字 replay 功能保留在 astra-media，未进入 SDK。此重构不修改 save/package 格式，也不增加兼容 provider 或动态 ABI。

## 验证

```bash
cargo test -p astra-text
cargo clippy -p astra-text --all-targets -- -D warnings
cargo test -p astra-media --test text_layout
```

原文字行为测试迁到本 crate，覆盖真实授权字体的 CJK/Arabic/emoji shaping、有序 fallback、竖排/ruby、wrap/ellipsis、glyph bitmap、资源事务/释放、缓存复用、字体替换和并发 single-flight。普通 Rust 测试直接运行，不启动 Headless。package 与 replay 的集成测试留在 astra-media。此验证证明库行为保留，不代表四平台产品或 Musica 完成验收。
