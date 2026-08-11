# 前置

如果LOCAL.md存在，那么，它是开发者个人的额外环境说明，请在读完本文件后再阅读它。

# 编程原则

用于减少常见 LLM 编码错误的行为指南。可根据项目需求与特定指令进行合并。

**权衡说明：** 这些指南偏向谨慎而非速度。对于简单任务，可自行判断适用程度。

## 1. 编码前先思考

**不要假设，不要隐藏困惑，明确权衡。**

在实现之前：

* 明确陈述你的假设。如果不确定，就提问。
* 如果存在多种理解方式，要列出它们——不要默默选择一种。
* 如果有更简单的方案，要提出。必要时提出异议。
* 如果有不清楚的地方，先停下来，指出问题并询问。

## 2. 简单优先

**只写最少代码来解决问题。不要写推测性功能。**

* 不添加未被要求的功能。
* 单次使用的代码不要做抽象化。
* 不要写未经要求的“灵活”或“可配置”功能。
* 不用为不可能出现的场景写错误处理。
* 如果 200 行代码可以缩减到 50 行，重写它。

问自己：“资深工程师会觉得这过于复杂吗？” 如果是，简化。

## 3. 精准修改

**只改必须改的部分。只清理自己造成的杂乱。**

在修改现有代码时：

* 不要去“优化”相邻代码、注释或格式。
* 不要重构未出问题的部分。
* 保持原有风格，即使你会写得不同。
* 如果发现无关的死代码，只需提出，不要删除。

当你的修改产生未使用代码时：

* 删除由你修改引入的未使用 import/变量/函数。
* 不要删除已有的死代码，除非被要求。

如果因为执行格式化器而产生了无关改动，这表明内核存在一些地方没有遵循格式化器的规则，
即使发生格式化的地方在允许写集之外，也可以接受这些变动，因为这事实上是在维护内核的风格一致性。
除非这对开发者本身通过git diff review代码时造成了困扰，否则不需要回退这些变动。

测试标准：每一行改动都必须直接对应用户请求。

### 关键注释

写代码时，对于未来阅读者无法仅通过代码形态理解的行为必须加注释。本库要求注释的情况包括：

* 不明显的约束、不变量，状态机转换，锁顺序，生命周期或唤醒/取消顺序；
* Linux/POSIX ABI 选择、errno 区别、标志兼容性、静默兼容性、故意不支持的特性；
* 临时兼容桥、已接受的限制、回退路径，或在后续阶段必须消失的代码；
* 特殊情况，移除看似无害但会改变外部可见行为或破坏测试的代码。

良好的注释解释代码为何如此设计，依赖什么不变量，以及何时可以修改或删除。不要写仅重复下一行代码的注释。

注释不是叙事填充。只有在注释能保留决策、约束、边界、失败模式或删除条件，否则可以省略。

### 内核代码形状约束

这些约束用于避免 agent 写出表面能跑、但状态来源和诊断边界不自然的内核代码。它们优先约束并发原语、调度、文件对象、任务状态、VFS/设备边界和 syscall 辅助层。

* **单一真相源。** 不要缓存能够从 owning object 直接推导出来的字段。只有在性能、稳定 snapshot、跨生命周期诊断身份三类需求之一成立时，才允许保留派生字段；字段旁必须说明真相源、是否允许 stale、以及它是否只服务诊断。便宜的一致性检查使用 `assert!`。
* **诊断字段要显式标注。** `owner`、`wait_id`、token id、debug label 等如果只用于日志、panic、review 或排障，字段旁必须说明它们不参与行为决策。纯诊断字段不得反向驱动状态机；一旦参与行为，它就是协议状态，必须进入类型设计、不变量说明或 RFC 文档。
* **状态所有权只能有一个中心。** 一个状态转换只能由一个 owner 负责。其它结构应持有能力对象、弱引用、token、handle 或 snapshot，不能制造并列真相源。`Latch`、`WaitState`、`Event::Listener`、fd/file/device state 等结构尤其要避免“task 里一份，辅助对象里又一份”的双重语义。
* **窄接口优先。** 下层只需要唤醒能力、文件能力、任务身份或上下文窗口时，不要传完整 `Task`、`File`、`FileDesc`、私有锁或内部容器。优先定义窄的 ctx、token、handle 或 owner API，让调用者无法依赖不属于它的内部状态。
* **内存分配服从自然代码形状。** 当前工程阶段不追求内核全局 allocation-free，也不追求所有内存分配都通过 fallible API 逐层传播。IRQ / IRQ-off 上下文允许简单、适度且有界的内核堆分配；适度内核堆分配失败时立即 panic 是可接受策略，调用者不必为了形式上的可恢复性机械增加错误分支。不得仅为消除分配或 OOM panic 引入侵入式容器、预分配池、镜像状态、额外 owner，或扭曲生命周期与控制流；优先选择所有权自然、容易分析的直接实现。只有具体的容量、时延、重入或 allocator side effect 风险才能成为收紧分配的理由；不可睡眠上下文仍不得引入 blocking / synchronous reclaim、普通锁、remote placement、复杂 `Drop` / callback 或日志格式化。ABI 已规定的资源耗尽、容量背压和其它明确可恢复结果仍按其 contract 返回，不得偷换为 panic。
* **断言策略要按 correctness 区分。** 轻量、局部、表示正确性不变量的检查使用 `assert!`，不要用 `debug_assert!`。`debug_assert!` 只用于昂贵扫描、统计诊断，或 release 路径不能承受的检查。cleanup / `Drop` 路径应先退订、释放或撤销发布状态，再用断言暴露 bug，避免 panic 放大泄漏或悬挂状态。如果fail-close会反过来要求内核实现新的功能或者代价很高，可以在注释中说明为什么不 fail-close，并做注释标记，便于后续 review。
* **临时桥和兼容层必须带退出条件。** 为阶段迁移、LTP 兼容或 ABI 缺口引入的临时字段、fallback、双路径分发和兼容 wrapper，必须说明保留原因、行为边界和移除条件，不能让后续开发者误以为它是长期抽象。
* **结构性拆分是设计维护，不是默认越界。** 简单小修不要为了整洁随手拆文件；但当一个文件已经混合 syscall ABI、核心状态机、设备/文件后端、测试兼容桥、锁/生命周期规则或多套 UAPI/internal 转换时，继续把新职责塞进去会固化错误 owner boundary。此时应先做模块边界判断：同一 owner 内、行为保持的目录化拆分（例如 `foo.rs` 拆成 `foo/{mod.rs, abi.rs, state.rs, ops.rs}`）是允许的结构维护；涉及 owner surface、public API、可见性策略或 shared contract 变化时，必须按 Implementation Boundary 的停止条件上报。
* **重要常量进入kconfig作为可配置项。**

### 模块拆分不是默认越界

当当前改动会继续向一个已经混合多类职责的文件中添加新职责时，agent 必须先做模块边界判断。若拆分满足以下条件，它属于结构
维护，可以在当前任务中执行：

- 行为保持不变；
- 仍在同一 owner / subsystem 边界内；
- 不扩大 public API、可见性策略或 shared contract；
- 只是把已有或本次必需的职责按 ABI、state、ops、lifecycle、compat、tests 等稳定角色分文件；
- 验证方式能证明调用路径和外部行为未改变。

如果拆分会移动 owner surface、改变公共接口、越过 Implementation Boundary、改变共享 contract 或引入新抽象层，必须先停止并上报理由、范围和验证计划。

### KUnit 与 validation 模块

KUnit的执行、并发握手、证明外推、cleanup和production shape以
[`KUNIT-EXEC` / `KUNIT-CONCURRENCY` / `KUNIT-PROOF` / `KUNIT-SHAPE`](docs/src/contracts/kunit/execution-and-proof.md)
为唯一规范。普通KUnit默认不触达live scheduling；只有scheduler/wait/kthread/kworker/timer/timekeeping/IPI等并发机制
本身是被测语义时才允许例外，并必须通过production lifecycle和显式phase/predicate/Event/token/completion/join闭合。
不得用固定次数的yield、schedule、tick等待或wall-clock sleep模拟happens-before；timer/timekeeping/timed-wait或
scheduler tick测试中的时间可以是被测语义，其它timeout只能作为failure bound。不得让production state/control flow/API
理解KUnit测试协议。

除 KUnit framework 本身外，不默认为一组测试新建 `kunit.rs`、`tests.rs` 或 `kunit_support.rs`。owner-local
KUnit 应放在被测语义文件末尾的 inline `#[cfg(feature = "kunit")] mod kunits`；跨多个子模块的 composition
测试放在其最低共同 owner 的 `mod.rs` 中。只有拥有独立编译边界、外部 consumer 或明确阶段生命周期的
validation/probe facade 才单独成文件，并按提供的能力而不是具体 test harness 命名。

长期 host fixture 可以保留独立 conditional validation module，但其中每个入口都必须有真实测试 consumer，且
不得进入 production dependency。为缺失 production consumer 临时建立的 probe 必须说明替换/删除 gate；正式
consumer 出现后，测试改走真实路径或删除重复 coverage，不能让 probe 自然沉淀为 production API。

### 类型命名规范

如非实体确实就是这样的名字或者具备业务属性或者承载明确领域含义，在类型名称中使用以下词语时需要认真考虑：

Value（值）
Data（数据）
Manager（管理器）
State（状态）
utils（工具）、misc（杂项）或某人的姓名首字母缩写

因为“所有东西都是值”，“所有类型都是数据”，“一切都有上下文”，“所有逻辑都在管理状态”。使用这些词并不能传达任何有区分度的信息。

倾向于使用“utilities（工具集）”“miscellaneous（杂项）”或姓名首字母缩写，本质上是没有正确分类，或者更常见的是过度分类的表现。这类声明可以直接放在需要它们的模块根部，不需要额外的命名空间。

## 4. 目标驱动执行

**明确成功标准，循环直到验证通过。**

将任务转化为可验证目标：

* “添加验证” → “先写无效输入测试，然后通过测试”
* “修复 bug” → “先写复现 bug 的测试，然后通过测试”
* “重构 X” → “确保重构前后测试通过”

对于多步任务，列出简要计划：

```
1. [步骤] → 验证: [检查内容]
2. [步骤] → 验证: [检查内容]
3. [步骤] → 验证: [检查内容]
```

明确的成功标准可以让你独立循环。模糊标准（“让它能工作”）则需要不断确认。

### 开发工作流与实现反馈

完整规则见 `docs/src/development-workflow.md`。开发按风险分为三档：Patch 默认只产生代码、测试和 Git/PR 证据；值得长期追溯的局部决策使用一份自描述 small change record，不强制同步双周日志；owner、ABI、shared contract、非平凡并发/生命周期、probe、多 cutover 或 target renegotiation 未闭合时使用 RFC。RFC 默认只有 `index.md`，其它 supporting pages 与 transaction devlog 按真实需要创建；positioning/backgrounds 不是晋级 RFC 的前置步骤。

小迭代默认使用一个 closure checkpoint；确需独立 review、commit 或授权停止点时，可以在同一个已完整解析的 Implementation Boundary 内使用至多两个 execution checkpoint。CKPT 1 必须独立安全，并对受保护 public API/ABI/visible semantics/current contract 保持中性；CKPT 2 完成整个 target，并承担至多一次最终 semantic/contract cutover。两个 checkpoint 共用一份 change record，不拆 `implementation.md` 或 transaction。普通 commit 不计入上限。需要跨 checkpoint 重新解析 target、owner、handoff、failure、cleanup、ABI、contract、acceptance 或验证，或者需要 probe、transitional contract、多个独立 cutover、超过两个正式 execution checkpoint 时，升级 RFC。用户只授权 CKPT 1 时，完成后停止。

默认使用语义级 `Implementation Boundary`，不维护逐文件 write set。边界必须说明 target/non-goals、owning subsystem、protocol/state owner、handoff、failure、cleanup、受保护的 public API/ABI/visible semantics/current contract、acceptance、验证 claim 和停止条件。预计文件或目录只能作为非穷举提示；边界内的 import/re-export、模块注册、同 owner 新文件、定向测试和行为保持型拆分可由 agent 自然闭合。用户显式给出的严格文件限制仍是本次任务的附加约束。

如果实现需要改变 target、owner、handoff、failure、cleanup、public API、ABI、visibility/shared contract、acceptance 或验证强度，或者要把无关问题纳入当前迭代，必须在完成声明或 cutover 前停止并上报。不得为了适配旧文件提示制造不自然的 adapter、重复状态或绕过路径。用户只授权某个 checkpoint/stage 时，完成后停止，不自动进入下一 gate。

正式 semantic gate 只用于独立 contract cutover、ABI 发布、owner 迁移、高风险 probe、不安全中间态或用户明确要求的 semantic gate；小迭代 execution checkpoint 仅按上文形成轻量停止点。未来 stage 只需保留目的、依赖和受保护边界；不得仅因缺少具体类型、算法、逐文件路径或精确命令形成 finding。probe 计划放在按需 `implementation.md`，说明 hypothesis、protected boundary、failure signal、write-back 和退出条件；probe 代码不能因“已经能跑”自然沉淀为长期抽象。

实现反馈不得自行改写 accepted target，但可以触发 `Target Renegotiation Gate`。真实证据表明原目标代价过高或只能形成较弱能力时，review 决定保持原目标、接受较弱但自洽的新修订、拆 follow-up RFC 或保持 Not Cut Over。agent 可以提交证据和 reduced-target 提案，不能自行批准；新 target 接受并完成对应 cutover 前，不得把更弱实现写成当前事实、accepted limitation 或原 target closure。

correctness invariant 约束唯一 owner、并发、生命周期、cleanup、内存安全和 ABI 诚实性，不能作为工程妥协项；target guarantee/capability 可以经 target renegotiation 修订；类型、helper、内部模块和数据结构属于 implementation preference。accepted limitation 必须位于新 target 之外，新 target 范围内的错误仍进入 open issues。

执行事实和验证优先留在 Git/PR；只有按分级确有长期记录价值时才写 change record 或 transaction。保持 target 的实施路线、stage 顺序、验证安排和停止条件更新按需 `implementation.md`；target/owner/ABI/contract/acceptance 变化进入 RFC review；effective shared rule 只在 cutover 时更新 current contract；接受限制和开放缺陷进入 register。不要创建通用 `feedback.md`、`probe.md`、`experiments.md` 或 `friction.md`。

每次 Patch、小迭代、checkpoint、stage 或 RFC 实现收口前必须执行 Architecture Friction Scan，检查第二份状态真相、owner 穿透、私有表示泄漏、为局部需求扩大 public API、调用者/架构/测试特判、无退出条件的临时桥、隐含 failure/cleanup 顺序、无真实义务的新抽象层，以及通过降低 oracle/validation/ABI 诚实性换取“跑通”。结论必须有具体代码路径、状态/owner/lifecycle 模型或已接受下一步作为证据；普通 import/module 注册、编译错误、工具链问题和未被本轮恶化的相邻技术债不构成摩擦。

没有具体摩擦或只剩 Safe 时不输出占位结论。未在当前边界内消除的 Euclid 在收口时简短报告证据、模型偏差、影响和最小修正方向；Keter/Apollyon 必须立即停止，不得声明完成或 cutover，并报告当前 diff/代码处置和需要的 owner/RFC/target 决策。Patch 中需要长期保留的摩擦提示升级为小迭代；小迭代中的未决 owner/contract/protocol 摩擦提示升级 RFC。

RFC 文本历史由仓库 Git 保存，不创建 per-RFC 仓库、版本化 canonical 副本或默认 amendment。`R0`、`R1` 只标记已接受 target 语义修订；措辞、证据、内部路线和文件布局调整不递增。历史 RFC、Completed transaction、manifest 和 change record 不批量迁移，新规则从新任务及活跃 RFC 的下一个未开始 gate 生效。

Contract 文档按 owner 和共同变化/共同证明的协议边界组织。`Contract Impact` 只列真实变化的 `Introduce`、`Refine`、`Replace`、`Remove`、`Scoped Exception`；未变化规则作为 Dependencies 链接，不登记 `Preserve`。Draft/Accepted target 不得提前覆盖 effective contract；只有达到 cutover 的验证和停止条件后才更新 current contract，证据可以来自原子 change record、RFC closure、Git/PR 或按需 transaction。

---

**这些指南有效的表现：** diff 中不必要的改动减少；因过度复杂化而重写的次数减少；在实现前先提问澄清，而不是在犯错后再询问。

## 测试流程

内核首先自己挂载启动盘，这个启动盘的构建目录是build/rootfs下的产物，配置见conf/rootfs。

内核会启动启动盘的init(anemone-apps/init)程序，init程序接着会启动一个user-test(anemone-apps/user-test)，这个user-test会进行挂载测试盘，接着chroot到测试盘下，接着初始化测试环境，然后开始执行测试脚本。

## 实现syscall时的准则

- 对于POSIX/linux也没有明确的设计或者corner cases，我们也没有义务给出兜底。
- 当代价过高时，我们允许语义强一致性做些让步。此外，如果有些flag难以实现，先打log，然后不支持或者静默兼容（如果用户观
  测效果不变）即可
- 对于特殊处理（比如静默兼容的flag）的实现，我们必须给出注释，说明 ABI 取舍、可见行为和移除条件，同时打日志以确保系统的可观测性。

## 开发前提

设计/实现之前，先阅读docs的register，这里有当前记录的开放问题和已知限制。

## 关于分数占比最大的测例LTP

### 输出解释
- **PASS**：测试用例成功通过。
- **FAIL**：测试用例失败，可能表示内核 bug 或环境配置问题。
- **TCONF**：测试用例不适用于当前系统（如缺少功能支持）。

### 评分依据

LTP 的评分依据主要基于测试用例的执行结果，旨在评估系统的功能正确性和稳定性。以下是评分的核心标准：

1.  测试结果分类
- **PASS (通过)**：
  - 测试用例按预期执行，功能正常，返回值为 0。
  - 评分：100%（该用例满分）。
- **FAIL (失败)**：
  - 测试未按预期执行，可能由于内核 bug、权限问题或硬件限制。
  - 评分：0%（该用例不得分）。
- **TCONF (不适用)**：
  - 测试用例因系统不支持相关功能而跳过（如旧内核版本）。
  - 评分：不计入总分。
- **BROK (中断)**：
  - 测试因外部因素（如资源不足）中断。
  - 评分：视情况分析，通常不计入总分。

2.  总体评分计算
在我们的比赛中，一个TPASS算一分。所以我们基本优先考虑那些子测例很多的测试，这样拿分高。

3.  评估标准
- **功能覆盖**：测试用例是否覆盖了目标功能的所有关键点。
- **稳定性**：在压力测试（如高负载、并发）下是否仍然通过。
- **可重复性**：多次运行结果是否一致。
- **错误信息**：失败用例是否提供清晰的诊断信息，便于定位问题。

1.  注意事项
- LTP 不是基准测试工具（benchmark），不直接衡量性能，而是关注功能验证。
- 测试结果受环境影响（如内核版本、硬件配置），需结合具体上下文分析。

## 端到端测试脚本

`scripts/run-user-test-rv(la)64.sh` 是端到端测试脚本。它会自动构建启动盘、把调用者
提供的测试盘 master 复制为当前 worktree 根目录下的运行副本、构建内核、启动 QEMU，
再由 init 启动 user-test 完成测试并自动关机。

脚本本身不加 `sudo` 执行；rootfs 默认直接构建。开发环境中的 libguestfs 需要提权时，
调用者通过 `--rootfs-sudo` 让 rootfs 构建阶段使用 sudo。

rootfs 配置由仓库跟踪的 `conf/rootfs/pretest-rv64.toml` 和
`conf/rootfs/pretest-la64.toml` 拥有，不再使用根目录下的 `rootfsconfig-*`。

调用形式为 `./scripts/run-user-test-rv(la)64.sh [--rootfs-sudo] <sdcard-image> [log-file]`。测试盘必须由
调用者显式选择，不能依赖无阶段含义的默认路径；个人环境中的初赛/决赛资源位置以
`LOCAL.md` 为准。日志参数可省略，默认写入 `build/user-test-rv(la)64.log`。

`scripts/run-final-test-rv64.sh` 和 `scripts/run-final-test-la64.sh` 用固定内嵌 BusyBox
启动决赛测试环境。基本调用形式为
`./scripts/run-final-test-rv(la)64.sh <sdcard-image> [log-file]`。

修改anemone-apps/user-test/ltp/profile.txt可以选择执行的测试组合，这样可以针对性地执行某些测试，或者跳过一些测试。
