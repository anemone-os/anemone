# 小迭代记录

小迭代记录保存不需要 RFC、但又比双周开发日志摘要更具体的修复、调查和局部实现事实。

它的职责是回答：

- 触发这次工作的症状、测试失败或观察是什么；
- 本次实际改了什么，不改什么；
- 验证到什么程度；
- 还有哪些风险、延期项或 register / current limitations 链接。

小迭代记录不是 backlog，也不是设计草案。未定稿的中大型方案应走私有草案或 RFC 工作流；跨多天、跨子系统、需要阶段 gate 或审计证据的实现应走事务日志。

## 命名与链接

- 文件放在 `docs/src/devlog/changes/`。
- 默认使用单文件：`YYYY-MM-DD-short-slug.md`。
- 如果需要背景材料，可以使用同名目录：`YYYY-MM-DD-short-slug/index.md`。
- 目录版记录可以包含 `backgrounds/`，用于保存证据摘要、Linux / LTP 对照、历史材料或运行记录。
- 双周开发日志保留一条短摘要，并在 `Related` 或 `Details` 中链接对应记录。
- register、current limitations、RFC 背景材料和事务日志可以按需链接小迭代记录。

## 单文件与目录边界

优先使用单文件，保持小迭代记录低摩擦。只有当单文件会变成难以扫读的证据包时，才升级为目录。

目录版记录仍以 `index.md` 为记录本体，回答 trigger、scope、change、validation、risk 和 links。`backgrounds/` 只保存事实材料，不定义计划、不变量、阶段 gate 或 review issue。

如果一个小迭代记录开始需要 accepted contract、不变量、阶段计划、tracking issues 或多轮文档层 review，它应升级为 RFC 工作流，而不是继续扩张 `changes/` 目录。

## 当前记录

暂无独立小迭代记录。既有双周开发日志保持原状，不回填拆分。
