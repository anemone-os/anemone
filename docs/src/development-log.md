# 开发日志

开发记录只保存以后无法从代码、测试和 Git 低成本恢复的事实。它不是每项开发的强制流水，也不承担 RFC target、current contract 或当前问题状态。

## 开发分级与默认记录

| 等级 | 适用范围 | 默认记录 |
| --- | --- | --- |
| Patch | 已确定语义的局部实现、修复、测试或结构闭包 | 无正式过程文档；代码、测试、验证和 Git/PR 足够 |
| 小迭代 | 值得长期保留的局部决策、根因、兼容取舍或调查结论 | 一份 change record |
| RFC | owner、ABI、contract、生命周期、并发、多个 cutover 或 target 仍需共享决策 | RFC `index.md`；其它页面和 transaction 按需 |

详细升级条件、Implementation Boundary 和反馈规则见[开发工作流](./development-workflow.md)。

## Patch

Patch 默认不写双周日志、小迭代记录或 transaction。文件数、commit 数和耗时不决定等级；只要行为已经由 current contract、现有代码/测试或明确任务目标确定，且没有新的 owner、ABI、contract 或 acceptance 决策，就可以保持 Patch。

必要的关键注释、回归测试，以及修复后对现有 register/current limitation 的更新仍应完成。若 Patch 暴露了值得长期保存的局部架构摩擦或兼容判断，升级为小迭代；若暴露未决 owner/contract/protocol，升级 RFC。

## 小迭代记录

小迭代记录位于 `docs/src/devlog/changes/`，默认使用单文件 `YYYY-MM-DD-short-slug.md`。它回答：

- Problem / Context：触发工作的问题或证据；
- Decision：选择了什么局部语义或处理方式；
- Change：实际发生的行为或结构变化；
- Validation：实际运行、用户运行和 Not Run；
- Remaining Risk / Links：仍有意义的风险和来源。

`Contract Impact / Cutover`、`Tracking Issues`、`Architecture Friction` 和 `backgrounds/` 都是按需内容。不要为了填模板保留空章节。

小迭代不再强制同步双周日志。新增记录只需加入[小迭代索引](./devlog/changes/index.md)和必要的 mdBook 导航；register、current limitations、RFC 或 issue 可以按需链接它。

contract-bearing small change 只允许一个已完整解析的原子 cutover。protocol/state owner、handoff、failure、cleanup、Implementation Boundary 和验证必须明确；effective 正文仍只位于 current contract。需要 probe、transitional contract、多个语义 checkpoint、target renegotiation 或未关闭 Apollyon/Keter 时升级 RFC。

如果记录后来升级 RFC、被证明有误或被 supersede，追加简短来源/更正链接；不要扩张为第二套 RFC，也不要为本地 issue 拆出独立 `implementation.md`、`invariants.md` 或 `tracking-issues.md`。

## 事务日志（按需）

transaction 只用于长期、多 checkpoint、多 cutover、probe/renegotiation 或需要独立 handoff 的执行历史。RFC 进入实现不自动触发 transaction。

使用 transaction 时：

- RFC 保存 accepted target、contract delta 和 Implementation Boundary；
- transaction 只追加 checkpoint、review、验证、cutover、更正、target renegotiation 和 handoff 事实；
- current contract 保存 effective shared rules；
- transaction index/SUMMARY 只提供导航，不再要求双周日志入口；
- Completed transaction 不因后续修订重新打开；需要独立长期历史时另建记录，否则使用 RFC closure 和 Git/PR。
- Terminated transaction表示维护者永久停止未完成执行：不是Completed，不保留active Next，也不得重开；
  已有证据保留，live defect/limitation链接register，未来相关工作作为独立任务重新分类并授权。

## 双周开发日志（可选）

双周 devlog 是人工选择的时间线摘要，不是 workflow gate。只有当一段时期的工作需要面向协作者提供时间入口时才写；Patch、小迭代、RFC、transaction 都不因存在而自动生成双周条目。

条目应短而事实化：Summary、Area、Validation 和 Related 通常足够。不要复制 change record、RFC、transaction、register 或 current contract 中已经存在的状态、验证矩阵和问题结论。

## 架构摩擦反馈

每次实现收口前都要内部扫描架构摩擦，但只有发现具体信号时才对外记录：

- Euclid 可以在实现完成后写入 final report、change record、RFC closure 或 transaction；
- Keter/Apollyon 必须在完成声明或 cutover 前停止；
- 没有摩擦或只剩 Safe 时，不写“无摩擦”占位结论；
- 不建立 `friction.md` 或全局摩擦台账。

具体证据要求和停止阈值见[开发工作流：架构摩擦扫描](./development-workflow.md#架构摩擦扫描)。

## Register 与历史

register/current limitations 只保存当前开放问题和接受限制。事项关闭后从 register 移除；长期历史由 Git、change record、RFC closure 或按需 transaction 保存。不要在 register 中保留 Closed/Neutralized 记录充当档案。

既有 devlog、change record 和 transaction 都是 legacy history，不批量重写为新模板。新规则适用于新任务和活跃 RFC 的下一个尚未开始 gate。

## 查询入口

- 查当前行为：代码、测试和 current contracts。
- 查局部决策或调查：小迭代记录。
- 查 accepted target、contract delta 和 acceptance：RFC。
- 查长期多 checkpoint 执行证据：按需 transaction。
- 查当前缺陷或接受限制：register/current limitations。
- 查时间线：可选双周 devlog 或 Git 历史。
