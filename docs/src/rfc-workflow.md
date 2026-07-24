# RFC 工作流

本文定义中型及以上功能迭代的文档生命周期。目标是让当前契约、变化提案、review 结论和实现证据都有稳定归属，避免实现阶段依赖个人草稿、聊天记录或临时上下文。跨 RFC 长期生效的规则另见 [当前契约](./contracts.md)，可复制形状见 [当前契约模板](./contract-template.md)。

RFC 工作流不是要求实现前消除所有不确定性。文档层 review 负责先闭合 accepted target、contract delta、正确性边界、最终证明义务和反馈入口；实现阶段负责用真实接口、状态流转、错误路径和集成结果反向校正 RFC。多阶段 RFC 接受时只要求第一个可执行阶段完整解析为 `Ready`，更远阶段保持 `Outline`：说明目的、依赖、受保护边界和后续解析触发点，但不伪造尚无证据支持的类型、算法、逐文件 write set 或精确测试命令。

实现反馈不得自行改写 accepted target，但可以触发显式的 `Target Renegotiation Gate`。如果工程证据表明原目标代价过高、无法形成合理实现，当前 gate 必须在 cutover 前停止并记录证据；随后由 RFC review 决定保持原目标、接受较弱但自洽的新修订、拆出 follow-up RFC，或保持 Not Cut Over。重新接受前，agent 不能缩小目标、降低验证结论、隐藏失败路径、把 correctness invariant 改成建议项，或让部分实现冒充原 target closure。

## Git 历史与语义修订

RFC 不建立独立 Git 仓库，也不保存 `index-v1.md`、`invariants-v2.md` 这类并列版本。整个 Anemone 仓库的 Git 历史负责保存文本 diff、review 和提交顺序；RFC 页首的 `修订` 字段只标记经过 review 接受的语义版本。

- Draft 尚未形成 accepted target 时，修订写 `Draft`。第一次接受进入实现时记为 `R0`。
- 只有目标、非目标、accepted target、target invariant、状态所有权、ABI / 外部可见语义、contract delta 或接受边界发生已接受的变化时，才递增为 `R1`、`R2`。措辞、链接、拼写、证据补充，以及不改变 target 的阶段顺序、write set 或验证命令调整不递增修订。
- Git branch / commit / PR diff 是待 review 的修订提案；只有结论被接受并写回 RFC target 后，才形成新的 RFC 修订。
- RFC 页首的 `状态` 描述当前修订。Closed RFC 的 `R1` 被接受但仍需代码实现时，状态回到 `Accepted for Implementation`；该修订验证收口后再回到 `Closed`。旧修订的关闭事实保留在修订记录和旧事务中，不因当前状态变化而重写。
- `index.md` 和 `invariants.md` 原地维护该 RFC 当前修订的 accepted target、contract delta 和 RFC-local proof obligations，不再承担跨 RFC 全局 current contract。`implementation.md` 保留已完成阶段并追加修订实施段；若当前计划与旧段冲突，应显式标记旧段由哪个修订取代。`tracking-issues.md` 保留问题来源、状态迁移和 neutralize 依据，不把后来发现的问题改写成初始设计已知事实。
- `index.md` 的修订记录只保存语义摘要、日期和对应 review / 事务证据。旧文本由 Git 恢复，不在 RFC 目录中复制快照。
- 已关闭 RFC 的新语义修订如果需要代码实现，应建立新的事务日志并引用目标修订；不要重新打开或继续延长已经 Completed 的旧事务。

既有 RFC 不因本规则批量补写修订记录。后续发生第一次语义修订时，再依据可验证的 accepted / closure 证据建立 `R0` baseline；无法从现有文档和 Git 历史确认的日期或结论不得猜测补齐。

如果一次变化已经改变 RFC 的核心目标、主要 owner、整体方案或大部分证明边界，使 target 文本不再能自然表达为同一设计，应新建 follow-up RFC，而不是继续提高旧 RFC 修订号。新 RFC 通过 contract ID 和来源链接声明 supersession；旧 RFC 正文不再是 current contract 的维护目标，不要求为了 backlink 逐份反向改写。

## 当前契约与 RFC 的边界

`docs/src/contracts/` 按稳定 owner 和 contract surface 保存已经生效、会被多个 RFC 或模块依赖的共享规则。它不是全仓库不变量百科，也不要求一次整理完整领域。既有 RFC 保持原状；后续 RFC 第一次跨文档复用、扩展或替换既有共享规则时，先提取本次变化所需的最小 contract 闭包：直接受影响的规则、唯一 owner，以及说明它们所必需的直接依赖。

RFC `invariants.md` 在涉及共享契约时分清三类内容：

- `Contract Impact`：按稳定 ID 声明 `Introduce`、`Preserve`、`Refine`、`Replace`、`Remove` 或 `Scoped Exception`，并写明生效 gate；未改变的规则只链接，不复制正文。
- `Target Invariants`：本 RFC 已提议或已接受、但尚未 cutover 的目标规则。
- `RFC-local Invariants`：只服务当前方案、probe、迁移桥、阶段原子性或验收的 proof obligations；它们不会自动进入长期 contract。

不要把所有期望都命名为 invariant。correctness invariant 约束状态唯一 owner、生命周期、并发、cleanup、ABI 诚实性等“实现若违反就不正确”的规则，不能作为工程降级项接受；target guarantee / capability 描述本 RFC 承诺的功能范围、兼容覆盖、原子性或性能边界，在当前修订中同样具有约束力，但可以经过 `Target Renegotiation Gate` 形成新的 accepted revision；类型、helper、内部模块和数据结构等 implementation preference 不属于 target invariant，应留给滚动阶段解析。

Draft 和 `Accepted for Implementation` 阶段不能把 target 写成当前事实。当前 contract 继续表达 effective 规则；accepted target 可以作为 `Pending Successor` 链接出现，但完整目标仍由 RFC 保存。只有 transaction 的 contract cutover gate 满足验证和停止条件后，才原子更新受影响的 contract ID、当前来源和生效证据。纯文档语义校正，或 RFC、实现、验证在同一个原子变更中完成时，接受点可以同时是 cutover。

`Introduce` 只用于此前不存在 effective contract 规则、由本 RFC 新增的 stable ID。它在 cutover 前只有 RFC target，没有可链接的 current authority；`Contract Impact` 的当前规则栏写 `None（尚未生效）`，cutover 时才在 `docs/src/contracts/` 创建 Active 条目。已有行为只是尚未提取到 contract 层时，不能因为“没有文档”而标成 `Introduce`；应先从 live owner、Closed RFC 和执行证据提取最小 effective baseline，再按真实语义使用 `Preserve`、`Refine`、`Replace`、`Remove` 或 `Scoped Exception`。

不变量按范围分流：局部实现约束优先使用 assertion、关键注释和定向测试；只服务单个 RFC 的规则留在 RFC；需要跨 RFC/模块引用的规则进入 contract surface。文档按 owner 和共同变化、共同证明的协议边界组织，不机械镜像源文件，也不为每条小规则单独建页。

跨领域规则如果只是依赖，使用 contract ID 引用，不复制对方规则；如果正确性依赖跨域 handoff、顺序、能力移交、取消或 teardown，建立接口级 contract，并明确唯一协议 owner、各份状态的唯一 owner和参与方局部义务。无法指出唯一 owner、两边都缓存同一可变状态或 cleanup 没有最终负责方时，属于文档层 blocker，不能写成“共同 owner”后进入实现。

## 适用范围

满足以下任一条件时，应走 RFC 工作流：

- 改动跨多个子系统，或会改变 shared contract；
- 改动涉及 ABI、兼容性、调度、等待、生命周期、锁序或资源所有权；
- 方案需要多轮 review，且 review 结论会影响实现顺序；
- 后续实现预计跨多天、多个 agent 或多个 checkpoint；
- devlog、register 或后续 RFC 需要长期引用该计划。

简单 bugfix、局部清理、一次性实验和不影响公共契约的小补丁不需要 RFC。它们可以只写普通 devlog、register 条目、小迭代记录，或不写正式文档。小迭代记录可以在单页内写清局部问题、解决方案和本迭代内部 tracking issues；如果这些内容开始需要仓库级 contract、不变量、阶段 gate 或多轮 review，再升级为 RFC。

## 生命周期

### 1. 私有草案

早期探索可以放在开发者自己的 gitignored 草稿区。私有草案可以提前采用 [RFC 模板](./rfc-template.md) 的目录形状，便于后续提升，但它不是公共 canonical source。

私有草案阶段允许快速重写、丢弃和拆分。公共文档、devlog 和 register 不应把私有草稿路径作为长期引用目标。

### 1.1 公共外部源码证据

RFC、small-change、transaction 或背景材料使用外部源码作为 ABI、可见行为或实现比较证据时，遵循
[外部源码引用规则](./external-source-references.md)。`xref:<source-id>:<repo-relative-path>#<locator>` 中的
source ID 必须来自仓库 tracked `xref/sources.toml`，路径从上游仓库根开始；引用不得依赖本机绝对路径、
私人 checkout 路径、branch、`HEAD` 或其它可移动版本。

私人参考可以用于探索，但不是公共 citation authority。一次性证据可以直接记录固定 commit 的普通
upstream permalink；只有长期反复使用、适合成为仓库公共参考的项目才进入 `xref/sources.toml`。外部源码
citation 只证明对应快照中的源码事实，不能替代 Anemone RFC target、current contract、live source 或执行验证。
既有 RFC 不批量迁移；后续第一次修改或重新依赖相关证据时再采用 canonical citation。

### 2. 草案成形

当方案开始稳定时，应按目录级 RFC 的角色拆分：

- `index.md`：入口状态、背景、目标、非目标、文档地图、方案摘要、接受边界和下一步；
- `invariants.md`：contract impact、target invariants、RFC-local proof obligations，以及本方案需要的状态所有权、线性化点、锁序、生命周期和禁止退化项；
- `implementation.md`：全局迁移约束、未来阶段 Outline、下一个 Ready 阶段的精确交付/审计/验证/退出条件、probe / vertical slice gate，以及滚动解析的 resolved write-set manifest；
- `tracking-issues.md`：当前 design review 或实现反馈发现，且仍影响实现顺序、review gate、停止边界或验收判断的设计问题；
- `backgrounds/`：历史材料、旧问题清单、旧计划、被拒绝方案和运行证据索引。

不是每个 RFC 都需要 `invariants.md` 或 `tracking-issues.md`。如果正确性不依赖复杂协议、也不改变共享 contract，可以省略不变量页；如果 review 没有发现需要持续跟踪的设计缺陷，可以不创建 tracking 页。涉及既有 contract ID 的 RFC 必须在 `index.md` 或 `invariants.md` 给出 `Contract Impact`，不能只在实现计划中暗示变化。

### 3. 文档层 Review

中型及以上迭代默认先在文档层闭合协议，再进入实现。这里的闭合指 accepted target、contract delta、正确性边界、最终证明义务和反馈入口已经明确，第一个可执行阶段已经达到 Ready；不表示所有未来阶段都必须在编码前被精确设计。review 应分清三类内容：

- 不变量和系统语义是否自洽；
- 子系统边界、状态所有权、生命周期、可观测性和失败路径是否合理；
- future Outline 的目的、依赖、受保护边界和解析触发点是否足够说明可达路径；第一个可执行阶段的交付、实现策略、review gate、验证、停止/退出条件和 resolved manifest 是否已经完整解析。

对明确标记为 `Outline` 的未来阶段，不得仅因缺少具体类型名、函数签名、逐文件路径、完整 corner-case 矩阵或精确测试命令而形成 finding。只有当 Outline 缺少必要依赖/受保护边界、无法说明 target 的可达路径，或把可能改变 owner、ABI、contract / acceptance boundary 的决定无 gate 地推迟时，才属于文档层问题。

若 RFC 影响共享 contract，文档层 review 还必须确认：影响表覆盖所有直接受影响 ID，提取范围已经形成最小闭包，owner 与跨域局部义务唯一，当前 effective 与 accepted target 没有混写，且每个 `Introduce` / `Refine` / `Replace` / `Remove` 都有明确 cutover gate。未登记的相邻领域不因为本轮顺手整理而进入 write set。

review 发现的 confirmed design issue 应写入 `tracking-issues.md`，并使用当前等级名：`Apollyon`、`Keter`、`Euclid`、`Safe`、`Neutralized`。不要把实现进度、普通 TODO 或历史讨论塞进 tracking issues。

进入实现不要求 `tracking-issues.md` 中所有条目清空。仍会改变 accepted target、contract delta、状态所有权、ABI 边界、阶段顺序或验收判断的 Apollyon / Keter 必须先 neutralize，或者明确转成某个实现 gate 的停止条件。Euclid / Safe、已接受延期项、以及只能通过实现证据验证的风险，可以带入实现阶段，但必须在 `implementation.md` 或 transaction devlog 中写明验证点和回写路径。

修复 design issue 时，必须把修复折回 `index.md`、`invariants.md` 或 `implementation.md` 的 target 文本；如果问题改变已经生效的共享 contract，还必须更新 `Contract Impact`，并在实际 cutover 时更新对应 contract ID。`tracking-issues.md` 只记录问题状态、修复依据和链接，不能替代主文档。

### 4. 提升为公开 RFC

当方案进入共享决策流程，或后续 devlog/register 需要引用计划时，必须把已收口草案提升到：

```text
docs/src/rfcs/<short-slug>/
```

提升不是简单复制。公共 RFC 应重写标题、状态字段、文档地图和接受边界，让 `docs/src/rfcs/<short-slug>/` 立即成为该提案和 accepted target 的 canonical source；它不因此覆盖 current contract。公共文档不应继续暗示私有草案才是权威。

提升时至少同步：

- `docs/src/rfcs.md`；
- `docs/src/SUMMARY.md`；
- RFC 目录内的相对链接；
- 受影响的 current contract 和 pending successor 链接（如果存在）；
- 必要的 `backgrounds/index.md`。

### 5. 引入事务 Devlog

RFC 进入实现阶段时，必须建立事务日志：

```text
docs/src/devlog/transactions/YYYY-MM-DD-<short-slug>.md
```

同时建立双向链接：

- RFC `index.md` 页首的 `事务日志` 字段链接到 transaction；
- transaction 页首的 `Canonical Plan` 链接回 RFC；
- `docs/src/devlog/transactions/index.md` 加入入口；
- 当前双周 devlog 追加一条入口摘要；
- `docs/src/SUMMARY.md` 加入 transaction 导航。

RFC 记录 accepted target、contract delta、边界和计划。当前契约记录 effective shared contract。transaction devlog 记录实际执行、checkpoint、review 结论、验证证据、contract cutover、更正说明、剩余限制和 handoff。

事务日志必须注明所实现的 RFC 修订，以及本事务计划 Introduce / Preserve / Refine / Replace / Remove / Scoped Exception 的 contract IDs 与 cutover gate。`R0` 的首次实现使用初始事务；Closed RFC 的 `R1` 及后续语义修订如果需要代码工作，应建立新的、可独立收口的事务。单纯补充执行证据或更正事务事实不产生 RFC 修订。

### 5.1 跨 RFC 功能入口

有些能力会被多个 RFC 分段覆盖。此时不要为“功能进度”新建一份并行账本，也不要把 transaction devlog 的阶段事实复制到另一个状态文件中。

推荐做法是提供一个轻量导航入口：

- 如果某个 RFC 是 umbrella / parent RFC，在该 RFC 的 `index.md` 中列出后续 RFC、事务日志、register / current limitations 链接；
- 如果没有明确 parent RFC，在 [公开草案与 RFC](./rfcs.md) 的“当前 RFC”或领域分组中聚合相关 RFC 链接；
- 导航入口只说明“该看哪些文档”和每个链接覆盖的范围，不重新记录阶段完成度、验证证据或剩余问题。

跨 RFC 功能的当前事实仍按原职责归属：已提取的 effective shared contract 写在 `docs/src/contracts/`，accepted target 和 delta 写在对应 RFC，执行进展与 cutover 证据写在 transaction devlog，开放问题和接受限制写在 register / current limitations。这样可以给读者一个 feature 级入口，同时避免多重真相源。

### 5.2 受控反馈与探针阶段

对高风险设计点，RFC 可以在正式语义阶段前安排 probe / vertical slice gate。它用于验证接口形状、owner boundary、状态机闭合、错误路径、性能或集成风险，而不是绕开 RFC 的长期实现。

probe / vertical slice gate 必须写清：

- 要验证的假设和失败信号；
- 受保护的目标、不变量和验收边界，说明哪些内容不得在 probe 中削弱；
- 受影响的 effective contract IDs，以及本 gate 只验证 target、还是会执行 contract cutover；
- 最小 write set、保持不变的 public API 和不得沉淀的临时路径；
- 验证 floor，例如定向 build、source audit、smoke、LTP 子集或用户运行证据；
- 退出路径：删除探针、把结果折回 RFC、升级为正式阶段，或登记为 limitation / open issue；
- 若探针发现 accepted target / current contract 错误，或只能以明显更高成本实现，应先停止并进入普通 RFC review / `Target Renegotiation Gate`；接受后再更新 RFC target、Contract Impact 和必要的 tracking issue。已经生效的规则只在批准的 cutover gate 更新，不能由 probe 静默覆盖。

默认不要为反馈机制新建通用 `feedback.md`、`probe.md` 或 `experiments.md`。probe / vertical slice gate 的计划格式写在 `implementation.md`，执行结果写在 transaction devlog 的阶段条目中。只有当 probe 产生的 Linux/LTP 对照、日志摘要、被拒绝方案或证据包已经让 `implementation.md` 难以扫读时，才在该 RFC 的 `backgrounds/` 下增加具体命名的证据文件，例如 `backgrounds/<topic>-probe-YYYYMMDD.md`。这类文件仍是证据材料，不承担阶段计划、accepted target 或 effective contract。

探针代码不能因为“已经能跑”就自然变成长期抽象。只有当 transaction devlog 记录了证据、RFC 已接受对应 delta，且长期共享规则完成 contract cutover 时，探针形状才可以进入后续正式阶段。

### 6. 实现阶段

实现必须按 RFC 和 transaction 中的阶段推进。阶段成熟度只使用以下四种状态：

- `Outline`：未来阶段；只冻结概括目的、前置依赖、受保护的 target / contract / correctness invariant、不得跨越的语义边界和解析触发点。
- `Ready`：下一个可执行阶段；交付、实现策略或明确的 probe、审计、可观测性、验证、停止/退出条件、contract cutover 和 resolved manifest 已完整解析，但尚未自动获得执行授权。
- `Active`：已经通过当前 RFC / transaction / 用户或编排协议要求的启动授权，正在执行。
- `Closed`：已经按本阶段自己的 review、验证和退出条件独立关闭。

RFC 接受时只要求第一个可执行阶段达到 `Ready`；更远阶段保持 `Outline`。future Outline 可以列出预计目录、模块、验证类别或物理范围，但这些内容只是解析输入，不是写入授权，也不得被 reviewer 当成已冻结设计。protocol/state owner、public ABI、shared contract、accepted target 和 acceptance boundary 仍应在 RFC target 层明确；具体 helper、内部 API、模块落点和逐文件清单可以留到对应阶段的解析 gate。

### 6.1 滚动式阶段解析

多阶段 RFC 默认使用滚动式阶段解析。任意时刻只要求下一个可执行阶段达到 `Ready`；单阶段 RFC，或后续工作完全机械且已经由 live-source audit 证明稳定的 RFC，可以提前解析多个阶段，但每个 Ready 阶段仍需独立授权才能进入 Active。

第一个可执行阶段在实现开始前完成解析。之后，Stage N 的关闭与 Stage N+1 的解析是两个独立 gate：

1. Stage N 先按自身 review、验证和退出条件独立关闭，不因 Stage N+1 仍是 Outline 而保持 Active。
2. `N -> N+1 Implementation Resolution Gate` 在 Stage N 关闭后执行只读 preflight，读取 live source、Stage N 实际 diff、review finding、模块边界、验证证据和仍有效的 RFC target / current contracts。
3. preflight 把 Stage N+1 从 Outline 解析为完整阶段：精确交付、实现路径或 probe、审计与可观测性、验证命令/证据、停止/退出条件、兼容桥删除点和 contract cutover。
4. 同一 gate 冻结 Stage N+1 的 `Resolved Write Set Manifest`：允许修改的现有文件、计划新建的文件/目录、文档回写面、validation-only 输入、不应触碰的边界，以及 integrator / reviewer 责任。
5. `implementation.md` 保存完整 Ready 阶段和 resolved manifest 的唯一权威；transaction devlog 只追加 preflight 证据、批准事实、生效点和链接，不复制第二份计划或 manifest。
6. 完整解析只让 Stage N+1 达到 Ready，不自动授权实现，也不允许跳过用户或编排协议要求的阶段启动批准。

解析 gate 不新增独立 `stage-plan.md`、`manifest.md`、`write-set.md` 或其它并列计划文件。future Outline 的自然收窄、扩大、拆分、合并、重排或模块重组属于滚动解析，只要不改变 accepted semantics，就不增加 RFC 修订，也不记录为 write set 扩展。若解析改变 target invariant、protocol/state owner、public API、shared contract、ABI、visible semantics 或 acceptance boundary，停止在解析 gate，进入 `Target Renegotiation Gate` 或普通 RFC review。

只有 Ready / Active 阶段的 resolved manifest 冻结后，越界修改才属于 write set 扩展。worker 不得先修改再追认；扩展批准前仍只能修改当前 manifest 中的范围。

既有 RFC 不因本规则批量重写已完成阶段、历史 manifest 或 transaction 记录。仍在实施的 RFC 从下一个尚未解析的阶段开始采用滚动阶段解析；历史 transaction 中已经存在的清单继续作为当时执行事实保留，不要求搬迁或删除。

### 6.2 Target Renegotiation Gate

实现反馈可能证明原路线不合适，也可能证明原 target 的工程代价明显高于预期。前者属于普通路线修正；后者允许提出 target renegotiation，但不能由实现者或 agent 自行生效。当前阶段必须先停止在 cutover 前，transaction devlog 记录：

- 触发反馈的真实接口、代码、测试或集成证据，以及具体成本来源；“实现太难”本身不是充分证据；
- 已完成能力、尚未完成路径，以及现有代码应保留、保持 dormant、删除还是拆入独立阶段；
- 受影响的目标、correctness invariants、target guarantees、状态/协议 owner、ABI、contract IDs、验收边界和验证 floor；
- 保持原目标、采用不同路线、接受 reduced target、拆 follow-up RFC 或 Not Cut Over 的可比较方案；
- 若建议 reduced target，它为何能形成独立有用、ABI 诚实、可验证且不固化错误 owner / abstraction 的能力；未支持输入如何稳定拒绝，临时桥如何观测和退出。

review 只能形成以下结论：

- `Route Correction`：保持 accepted target，只更新 `implementation.md` 和 transaction，不增加 RFC 修订。
- `Accepted Reduced Target`：核心目标、owner 和总体方案仍属于同一 RFC，但能力或保证降低；原地更新 `index.md` / `invariants.md` / `Contract Impact`，递增 RFC 修订，再解析对应实施阶段。
- `Follow-up RFC`：核心目标、主要 owner、总体方案或大部分证明边界已经改变；当前 RFC 记录 Not Cut Over / partial result，由新 RFC 承接。
- `Not Cut Over`：现有部分实现不能形成安全、诚实、独立的能力；不得为了挽救沉没成本激活它，可删除或保持明确 dormant。

correctness invariant 不能作为工程妥协项：双重真相源、未闭合生命周期/cleanup、丢失唤醒、内存安全、并发正确性和 ABI 撒谎不能通过登记 limitation 合法化。target guarantee / capability 可以经 review 降低，但在新修订接受前仍是约束；implementation preference 可以在保持 target 时直接调整。accepted limitation 必须位于新 target 之外，仍落在新 target 内的错误进入 open issues。

Target Renegotiation Gate 不更新 effective contract。只有新 revision 已接受、对应实现完成，并达到新的 contract cutover gate 后，才能切换 current contract。未经接受的 reduced target 仍只是提案，不能作为当前行为、阶段 closure 或验证成功写入。

实现阶段可以安排独立的结构维护 gate。这个 gate 只做同一 owner 内的行为保持型拆分、模块目录化、可见性收窄、导入路径调整和调用面不变的文件搬移；不应顺手改变 syscall 语义、状态机、不变量或 ABI。推荐验证 floor 是 `git diff --check`、`just build`，以及必要的 `rg` 检查，确认外部调用面没有被扩大、旧兼容入口没有被误保留。

当拆分需要移动 owner surface、改变 public API、引入新的 facade、调整 shared contract，或扩大当前已冻结的 resolved manifest 时，不能把它包装成普通整理；应走 write set 扩展申请，并在 `implementation.md` 更新 manifest，在 transaction devlog 记录批准事实和结构边界。

阶段推进、review 结论、验证证据和 contract cutover 写入 transaction devlog。RFC 只在 accepted target、delta 或实施 gate 变化时更新；如果实现发现 RFC target 不可行、current contract 的不变量/边界错误，或只能形成较弱但可能有用的能力，应先停止 cutover，并通过普通 RFC review 或 `Target Renegotiation Gate` 决定后再继续。

实现期反馈按影响分流：

- 不改变 accepted target 或 effective contract 的实现事实、checkpoint、review 结论和验证结果，只追加到 transaction devlog；
- 保持 accepted target 的阶段 Outline/Ready 解析、阶段顺序、write set、验证安排、review gate 或停止条件变化，更新 `implementation.md`，并在 transaction devlog 记录原因和生效点；
- 改变 target invariant、状态所有权、ABI 边界、外部可见语义或接受边界时，先进入 RFC review / `Target Renegotiation Gate`；接受后更新 RFC `index.md` / `invariants.md` 的 target、`Contract Impact` 和必要的 `tracking-issues.md`；
- 只有 cutover gate 已达到 RFC 规定的验证和停止条件时，才更新 effective contract；同一阶段必须在 transaction devlog 记录受影响 ID、旧/新规则、agent/user 验证和生效范围；
- 已接受但暂不补齐的能力缺口进入 current limitations；本应工作但当前不正确的事项进入 open issues；
- 无法分类的实现摩擦不能静默用兼容层绕过，应先停止在当前 gate，补充证据后再选择上述归属。

如果反馈形成了已接受的目标、target invariant、owner boundary、ABI / 可见语义或接受边界变化，更新 RFC 文本时必须递增 RFC 修订，并在 `index.md` 的修订记录中链接对应 review / transaction 证据。只改变实现路线或验证安排、但保持 accepted target 的反馈不递增修订。纯文档语义校正可以没有代码 transaction，但必须保留 review 或小迭代记录入口；若它同时改变 effective contract，接受点就是 docs-only cutover，不能制造空事务或把未生效提案写成当前规则。

以下行为不属于有效反馈，必须停止并回报：未经 `Target Renegotiation Gate` 批准就缩小原目标、调低验证结论、隐藏失败路径、把 correctness invariant 改成建议项、用日志或静默兼容替代约定行为、把 Keter / Apollyon 重新命名成实现限制，或在未更新 RFC target / Contract Impact、未完成对应 contract cutover 的情况下让代码接受更弱语义。

write set 是协作边界，不是架构边界。写入型 agent 不能静默越过当前已冻结的 resolved manifest；但如果更合适的架构需要触碰新的 owner surface、移动 shared contract，或把 helper 放到更自然的子系统，agent 应停止并向总控或用户汇报扩展申请，而不是在原 manifest 内做兼容性绕路。

write set 扩展申请至少说明：

- 为什么原 resolved manifest 会导致错误分层、重复状态、旁路路径或不可维护的适配层；
- 需要新增的文件、模块或子系统边界；
- 对 RFC accepted target、contract delta、阶段 gate、review gate 和验证 floor 的影响；
- 批准后由谁集成，以及 transaction devlog 或 orchestration 文档中的记录位置。

扩展通过后，应先更新 `implementation.md` 中的 resolved manifest，再由 transaction devlog 记录批准事实、生效点和链接，然后继续实现。扩展未通过前，worker 仍只能在原 manifest 内修改，或保持停止状态等待人工决策。

### 7. 收口

事务完成时应更新：

- RFC 状态、当前修订和收口说明；
- 所有受影响 contract IDs 的 effective / pending / Not Cut Over 状态与来源；
- transaction devlog 的 `Status`、`Closure` 或最终阶段条目；
- 当前双周 devlog 的必要摘要；
- register 或 current limitations 中受影响的开放问题和限制；
- `tracking-issues.md` 中仍开放或已 neutralize 的问题状态。

收口记录必须区分：已实现能力、仍接受的限制、用户侧验证、agent 侧验证、未运行的验证。

## Artifact 边界

| Artifact | 职责 | 不应承担 |
| --- | --- | --- |
| 私有草案 | 快速探索、预 review、未公开方案打磨 | 公共 canonical source |
| Current contract | 已经生效的跨 RFC / 跨模块共享规则、唯一 owner 和来源 | Draft target、迁移计划、执行进度、全领域不变量普查 |
| RFC `index.md` | 状态、范围、方案摘要、accepted target、contract delta、接受边界、文档地图 | current shared contract、阶段流水账 |
| RFC `invariants.md` | Contract Impact、target invariants、RFC-local proof obligations | 整个领域的 current contract、实现步骤细节 |
| `implementation.md` | future Outline、Ready 阶段、resolved write-set manifest、resolution / probe / renegotiation gate、验证和停止条件 | 已执行 checkpoint 日志、第二份计划或 manifest authority |
| RFC 修订记录 | 已接受语义版本、变化摘要和事务入口 | 文本快照、逐 commit 历史、并列 canonical 版本 |
| Probe / vertical slice gate | 验证高风险假设、接口形状和集成风险 | 没有 RFC 回写的长期抽象或隐藏实现阶段 |
| 结构维护 gate | 同一 owner 内行为保持的模块拆分、目录化、可见性收窄和调用面确认 | 借整理名义改变语义、移动 owner surface、扩大 public API |
| `tracking-issues.md` | 当前 design-review issue 与实现反馈暴露的 design issue 状态 | 普通 TODO、实现进度、历史归档 |
| `backgrounds/` | 历史材料、旧计划、被拒绝方案、证据索引 | 覆盖 canonical 结论 |
| Transaction devlog | 执行事实、stage-resolution / write-set 证据、target renegotiation 提案与决定、checkpoint、验证、contract cutover、handoff | 未经 RFC 接受自行重新定义 target / effective contract、复制第二份权威计划或 manifest |
| 跨 RFC 功能入口 | 相关 RFC / transaction / register 的导航索引 | 阶段进度账本、验证事实副本、第二套问题状态 |
| 双周 devlog | 入口摘要和重要结论 | 长篇阶段日志 |
| Register / limitations | 当前开放问题和接受限制 | 设计草案或实现计划 |

## Agent 约束

Agent 处理 RFC 工作流时应优先读取本文、[当前契约](./contracts.md)、[RFC 模板](./rfc-template.md) 和 [当前契约模板](./contract-template.md)。如果任务涉及 review 输出，还应使用当前 Anemone review 等级。

Agent 可以帮助整理私有草案、执行文档层 review、修复 RFC 文本、提升公开 RFC、建立 transaction devlog 和更新导航。但 agent 不应把私有草稿路径写入公共 canonical 链接，也不应在用户明确要求文档层闭合时提前开始代码实现。

Agent 不应把 “Accepted for Implementation” 解释成所有风险已经消失，也不应把 “tracking issues 仍有 Euclid / Safe / gated item” 解释成实现必须停止。正确动作是检查当前 gate 的 blocker 是否已 neutralize、剩余不确定性是否有验证点和回写路径，再按 transaction devlog 推进。

Agent review 必须先读取阶段成熟度。对 future `Outline`，缺少具体类型、函数、算法、逐文件路径或精确测试命令是刻意延迟解析，不得单独形成 finding；review 只检查其目的、依赖、受保护边界、解析触发点和 target 可达性。只有下一个 `Ready` 阶段需要达到可直接执行与审查的精度。

Agent 可以基于工程证据提出 target renegotiation，但无权自行批准或把 reduced target 写成当前事实。它必须保持 current gate 未 cut over，明确 correctness invariant 与 target guarantee 的边界，并等待 RFC review 选择 Route Correction、Accepted Reduced Target、Follow-up RFC 或 Not Cut Over。

Agent 修改 RFC 时应先判断是否改变 accepted semantics，以及是否影响已经登记的 current contract。语义不变时保留当前修订；target 语义变化经接受后递增 `R<n>`，原地更新 RFC `index.md` / `invariants.md`，增量更新 `implementation.md` / `tracking-issues.md`，并为 Closed RFC 的新代码工作建立新事务。effective contract 只能在 docs-only 或 implementation cutover gate 更新。不要创建 per-RFC Git 仓库、版本化 canonical 文件副本或默认 amendment 文档。

Agent 不得为了启用 contract 层批量整理既有 RFC。遇到第一条跨 RFC 依赖或 supersession 时，只提取当前变化的最小闭包；contract 文件按 owner/协议边界组织，小规则聚合为稳定 ID 条目。跨域 contract 必须明确协议 owner、状态 owner、handoff 和 cleanup，不能用共享 ownership 掩盖双重真相源。

在实现文档或 agent 编排中，agent 应把 write set 视为默认协作合同。遇到必须越界的架构依赖时，正确动作是提交扩展申请并等待批准；不得自行扩大范围，也不得为了服从旧 write set 引入错误 owner boundary 或长期 compatibility layer。
