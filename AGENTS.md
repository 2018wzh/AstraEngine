# AstraEngine 工作规则

## 权威入口

- [总体架构](Docs/product/architecture.md)：产品职责、技术栈与平台目标。
- [重构契约](Docs/contracts/rebuild.md)：接口、所有权和生命周期；修改相关模块前读取对应章节。旧契约仅描述未迁移实现，冲突时以重构契约为准。
- [实施计划](Docs/status/implementation-plan.md)：当前范围、执行顺序、授权与验收；[性能审计](Docs/status/performance-upstream-audit-2026-10-12.md) 保存已有测量和上游问题。

## 实施

- 主线程负责契约、拆分、整合与最终验收，可委派独立子任务。每个实施实例独占 worktree、branch、target 和服务，共享文件指定唯一整合者；不编辑、编译或清理其他实例工作树，不冒报代理模型。
- 保留来源分支、用户修改和既有成果。常规实现、检查与本地提交已授权；远端操作按实施计划中的授权执行，破坏性操作另行确认，不改写已发布历史。
- 先改共享契约，再迁移全部相关消费者，删除被替代的旧链，不留兼容双轨。范围排除项以实施计划为准，不擅自扩大重构。
- 复用成熟依赖和已有机制，按职责与实际消费者拆分，合并纯转发层；避免帧内全量复制、序列化和无消费者抽象。Rust 类型是 schema 真源；跨平台脚本用 Python，命令示例用 bash/sh。
- 明确报告缺失能力和错误，不以 fallback、静默跳过或假数据掩盖。上游功能缺陷与嵌入适配分别处理，保留许可证、修改归属及精确依赖版本。

## 数据与资源

- 商业资源、正文、截图、存档、私有 profile/key 和转换产物只留 ignored 工作区；不误覆盖原商业存档，读取失败不回写原文件。
- 容器校验保留版本、长度/边界、唯一 section 和内容完整性；postcard struct 不使用破坏二进制读取的 `skip_serializing_if`。
- 日志复用 tracing/astra-observability，库不另建文件 sink；私有路径、secret 和商业正文不进入可提交报告。
- 读档、重开和退出使旧异步结果失效；关闭即使遇错也须完成取消、worker 回收和资源释放。

## 验证与记录

- 代码改动运行文档检查、fmt、受影响 crate 的 Clippy 与相关测试；阶段末检查活动 workspace build/clippy/test，并实际运行产品的改动路径。
- 视听验收使用实际硬件 GPU/音频。Headless、软件 adapter、NullAudio、首帧和 synthetic fixture 仅证明各自覆盖范围，不能替代实际交互、媒体、保存及结局验收；失败、上游阻塞和未覆盖项如实记录。
- 文档改动运行 `python Tools/check_docs.py`，不冒报产品验证。文档以中文为主，只维护当前契约、状态和下一步；历史留 Git，不叠加过程页或审批矩阵。
