# 开发日志

## 为什么要共享日志

在两位开发者协作时，最常见的调试问题通常是“这个子系统什么时候改过、改了什么”，而不是“某个人具体做了什么”。

因此，应该维护一条规范性的共享日志流，并在条目里标注作者，而不是各自维护一套正式日志。

## 存放约定

- 规范日志文件放在 `docs/src/devlog/` 下。
- 按双周建文件，例如 `docs/src/devlog/2026-05-11_to_2026-05-24.md`。
- 记录按时间顺序追加，不回写历史。
- 如果某项小迭代需要比双周条目更长的公开说明，应在 `docs/src/devlog/changes/` 下建立小迭代记录，双周开发日志只保留短事实摘要与链接。
- 对跨多天、跨子系统、需要阶段性审计证据的大型重构，可以在 `docs/src/devlog/transactions/` 下建立事务日志。双周 devlog 只保留该事务的入口摘要。
- 如果一项 RFC 已进入实现阶段，必须建立对应事务日志；RFC 保存 accepted target、contract delta 和计划，事务日志保存实际执行、contract cutover 和验证证据，已经生效的共享规则写入 current contracts。

这样日常维护简单，同时也允许深度排障材料独立存在。

## 双周开发日志

双周开发日志是共享时间线，不是完整调查材料的堆放处。它应该能让读者快速扫出最近发生过什么、属于哪个领域、验证到什么程度，以及进一步材料在哪里。

普通条目仍可以直接写完整字段；如果一次小迭代已经有独立小迭代记录，双周条目应压缩为：

- 一句话说明发生了什么；
- `Area` 或标题中体现 owning surface；
- `Validation` 写明已运行、用户运行、或未运行的验证；
- `Related` 链接对应的小迭代记录、current contract、register、RFC、事务日志或 issue。

不要在双周开发日志中展开长篇问题分析、多轮尝试过程、大段日志、完整 review 结论、checkpoint handoff 或未来执行计划。这些内容应进入小迭代记录、事务日志、RFC 或 register。

## 小迭代记录

小迭代记录用于承载不值得开 RFC、但又不适合塞进双周日志的一次局部迭代。它必须是自洽、可自描述的记录：读者只打开这一页，就能理解问题是什么、为什么按当前方案处理、实际推进到哪里、验证到哪里、还剩哪些局部风险。

小迭代记录不只记录完成后的事实。对于仍在推进的小问题，它可以在记录内部维护 `Problem`、`Solution` 和 `Tracking Issues` 章节，用来说明本轮问题、局部方案、review concern、验证缺口和关闭依据。但这些 tracking issues 只服务于当前小迭代本身，不承担仓库级 accepted target / current contract、跨子系统不变量或长期阶段计划。

适合建立小迭代记录的情况：

- 修复 LTP、user-test、兼容性或 ABI 边界问题，后续可能需要追溯当时判断；
- 一次 bugfix 的触发条件、根因、验证或剩余风险超过双周日志的合理长度；
- 小功能或局部清理影响明确 owning surface，且需要说明不改什么；
- 一次调查没有进入 RFC，但产出的分类结论会影响后续诊断；
- 一个局部问题需要先写清问题、解法和少量待关闭事项，确认它不值得启动 RFC；
- register / current limitations 需要链接到更具体的修复或调查事实。

不需要建立小迭代记录的情况：

- 纯格式化、局部重命名、注释修正或没有语义变化的清理；
- 三五句话就能在双周日志里说清楚的简单修复；
- 已经由 RFC 事务日志完整承载的阶段性实现；
- 未定稿的中大型方案；这类内容应先走私有草案或 RFC 工作流。

维护规则：

- 默认使用单文件，文件名为 `YYYY-MM-DD-short-slug.md`，放在 `docs/src/devlog/changes/`。
- 当记录需要背景材料时，可以升级为同名目录：`docs/src/devlog/changes/YYYY-MM-DD-short-slug/index.md` 是记录本体，`backgrounds/` 保存证据摘要、Linux / LTP 对照、历史材料或运行记录。
- 双周开发日志追加一条短摘要并链接小迭代记录。
- 小迭代记录可以被 register、current limitations、RFC 背景材料或后续事务日志引用。
- `Tracking Issues` 章节可以记录本迭代内的 review concern、方案缺口、验证缺口和关闭依据；问题关闭后应把结论折回 `Solution`、`Change`、`Validation` 或 `Risk / Follow-up`，不要只在 tracker 中留下最终语义。
- 如果小迭代后来升级为 RFC，原记录保留事实历史，并在 `Status`、`Follow-up` 或 `Tracking Issues` 中标明被哪个 RFC 或事务日志取代。
- 如果记录后来被证明有误，追加更正说明；不要静默改写已经完成的事实判断。

适合升级为目录的小迭代记录：

- 需要保留 Linux、LTP、用户日志或当前实现对照；
- 需要多份验证证据或运行摘要；
- 调查结论没有进入 RFC，但背景材料会被后续反复引用；
- 单文件已经影响扫读。

目录形态只是让小迭代记录容纳证据包和局部跟踪，不是小型 RFC。`index.md` 仍是唯一的自描述记录本体；`backgrounds/` 只保存证据摘要、Linux / LTP 对照、历史材料或运行记录。如果记录开始需要仓库级 accepted target / current contract、非平凡不变量、跨阶段实施计划、独立 `tracking-issues.md`、多轮文档层 review 或多个 agent/checkpoint 编排，应升级为 RFC 工作流，而不是继续扩张 `changes/` 目录。

## 事务日志

事务日志用于记录一次大型重构或长期迁移从启动到收口的完整状态，而不是替代每日开发日志。

适合开事务日志的情况：

- 改动跨越多个子系统，且阶段之间存在明确前置条件。
- 需要保留不变量、实现顺序、旁路审计、验证证据或回滚边界。
- 单个双周日志条目会过长，且后续多次更新都需要引用同一上下文。

维护规则：

- 文件名使用 `YYYY-MM-DD-short-slug.md`，放在 `docs/src/devlog/transactions/`。
- 双周 devlog 只追加入口记录，后续阶段推进优先更新事务日志。
- RFC 驱动的事务日志必须链接回对应 RFC；对应 RFC 的 `事务日志` 字段也必须反向链接到该事务日志。
- RFC 驱动的事务日志必须注明目标修订。Closed RFC 的后续语义修订需要代码工作时建立新事务，不重新打开或继续延长旧的 Completed 事务。
- RFC 驱动的事务日志必须列出受影响 contract IDs、变化类型和计划 cutover gate；没有则明确写 `None`。
- 每次更新只追加新的事务条目，不静默改写已经完成的阶段结论；确需更正时追加更正说明。
- 实现期反馈先写入事务日志。若反馈改变阶段顺序、write set、验证 floor 或停止条件，同步更新 RFC `implementation.md`；若反馈改变 accepted target、不变量、ABI 边界或验收判断，同步更新 RFC target / `Contract Impact` 和必要的 `tracking-issues.md`。只有达到批准的 cutover gate 后才更新 effective contract，并在同一事务条目记录证据。
- 事务日志收口后，保留最终状态、验证证据和剩余限制链接。

## 双周记录的常用字段

- `Date`
- `Authors`
- `Area`
- `Summary`
- `Motivation / Symptom`
- `Change`
- `Validation`
- `Follow-up`
- `Related`

如果双周条目只是指向小迭代记录或事务日志的入口摘要，可以省略 `Motivation / Symptom`、`Change` 和 `Follow-up` 中已经由目标页面承载的细节，但必须保留足够的摘要、验证状态和链接，让读者不打开目标页面也能判断这条记录的大意。

## 协作规则

- 一个任务通常只写一条规范记录，即使两位开发者都参与了。
- 如果一个任务跨多天推进，就写多条记录，而不是持续改写旧条目。
- 如果旧记录后来被证明有误，追加一条更正记录，不要静默篡改历史。
- 如果多人同时改同一个双周文件导致冲突过多，再考虑临时拆成更小周期，但默认仍以双周为单位。

## 质量门槛

开发日志最有价值的部分，是那些事后很难低成本恢复的事实：

- 最初的症状或动机是什么；
- 实际改了什么；
- 用什么命令、测试或复现步骤验证过；
- 还有哪些风险、不确定性或后续事项。

不要把开发日志写成流水账。短而事实化的条目寿命更长。

查询时优先按职责选择入口：

- 看最近时间线：双周开发日志。
- 查一次小修、小调查或局部语义变化：小迭代记录。
- 查仍然生效的问题或接受限制：register / current limitations。
- 查已经生效的跨 RFC 共享规则：current contracts。
- 查中大型 target、contract delta 和实现阶段证据：RFC 与事务日志。
