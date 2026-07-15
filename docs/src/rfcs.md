# 公开草案与 RFC

公共仓库中的草案只用于共享评审尚未定稿、但已经需要协作讨论的方案。

不是所有个人草稿都需要进入仓库；只有当一个问题已经进入共享决策流程时，才需要公开草案页面。

## 什么时候需要公开草案

满足以下任一条件时，适合创建公开草案：

- 方案影响多个子系统；
- 方案会改变 ABI、兼容性或外部契约；
- 方案需要跨人、跨时间异步评审；
- 方案预计会经历多轮讨论，且结论需要长期追踪。

## 存放方式

公开草案统一放在 `docs/src/rfcs/` 下，并默认使用目录级 RFC：

```text
docs/src/rfcs/<short-slug>/
  index.md
  implementation.md
  invariants.md          # 可选；协议、不变量或证明义务复杂时使用
  tracking-issues.md     # 可选；review 或实现反馈发现设计问题时使用
  backgrounds/           # 可选；保存历史背景、问题清单和被拒绝方案
    index.md
```

`index.md` 是总入口，负责说明状态、范围、文档地图、接受边界和下一步。`implementation.md` 是实现计划，负责记录阶段、审查合同、验证、反馈 gate 和停止边界。`invariants.md` 只在正确性依赖明确协议、不变量或证明义务时创建；调度、等待、锁序、生命周期等子系统通常需要它。`tracking-issues.md` 只在存在一组需要持续 review、分级和关闭的设计问题时创建；这些问题可以来自文档层 review，也可以来自实现期反馈。`backgrounds/` 只保存背景材料，不作为当前 canonical 结论的来源。

## RFC 修订与 Git 历史

RFC 不需要独立 Git 仓库。整个 Anemone 仓库的 Git 保存文本 diff、review 和提交历史；RFC 自身只用 `R0`、`R1`、`R2` 标记已经接受的语义版本。Draft 尚未形成 accepted contract 时写 `Draft`，第一次接受进入实现时形成 `R0`。

只有目标、非目标、accepted contract、不变量、状态所有权、ABI / 外部可见语义或接受边界发生已接受的变化时才递增修订。普通措辞、链接、证据补充，以及保持 contract 的阶段顺序、write set 或验证调整不递增。Git diff 是待 review 的修订提案，merge 后的 canonical `index.md` / `invariants.md` 是当前 consolidated contract；不要创建 `index-v1.md`、独立 amendment 文件或要求读者回放 patch 链。

`implementation.md` 和 `tracking-issues.md` 保留增量历史：已完成阶段不静默重写，后来发现的问题保留来源、状态迁移和 neutralize 依据。Closed RFC 的新语义修订需要代码实现时，应建立引用目标修订的新事务，而不是继续延长旧的 Completed 事务。完整规则见 [RFC 工作流](./rfc-workflow.md#git-历史与语义修订)。

RFC 页首 `状态` 描述当前修订：后续修订已接受但仍需实现时回到 `Accepted for Implementation`，该修订收口后再回到 `Closed`；旧修订的关闭事实留在修订记录和旧事务中。既有 RFC 不因新规则批量补写 `R0`，只在后续真实语义修订时依据可验证历史建立 baseline。

大型重构应把不变量、实现顺序、历史材料拆成同一目录下的子文档，避免 devlog 或 register 直接引用个人草稿。

RFC 被接受进入实现阶段，不表示所有未知都已经消除。它表示当前 accepted contract、验证 floor、停止条件和反馈入口足够明确。只能通过真实接口、状态流转、错误路径或集成结果验证的风险，可以作为受控 probe / vertical slice gate 带入实现；如果证据改变 accepted contract，必须回写 RFC canonical 文本。

不要为 probe / feedback 默认新建通用 `feedback.md`、`probe.md` 或 `experiments.md`。probe 计划写在 `implementation.md`，执行反馈写在 transaction devlog；只有证据包过长时，才在 `backgrounds/` 下增加具体命名的证据文件。

反馈只能优化路线，不能篡改目标或私自削弱不变量。如果实现期证据说明目标、不变量、ABI 边界或验收条件需要改变，应先回到 RFC review 并更新 canonical 文本；在此之前，代码和事务日志都不能把更弱语义当作已接受结果。

每个 RFC 入口都应在页首明确给出：

- `状态`
- `修订`
- `负责人`
- `最后更新`
- `领域`
- `开放问题`
- `下一步`

完整生命周期见 [RFC 工作流](./rfc-workflow.md)。可直接复制的草案结构见 [RFC 模板](./rfc-template.md)。

## 实现期事务日志

RFC 一旦进入实现阶段，必须建立对应的事务级 devlog：

```text
docs/src/devlog/transactions/YYYY-MM-DD-<short-slug>.md
```

同时更新：

- RFC `index.md` 页首的 `事务日志` 字段；
- `docs/src/devlog/transactions/index.md`；
- 当前双周 devlog，只追加该事务的入口摘要；
- `docs/src/SUMMARY.md`，让 RFC 和事务日志都出现在 mdBook 导航中。

事务日志记录实际执行、checkpoint、review 结论、验证证据、实现期反馈、剩余限制和更正说明；RFC 记录计划、边界和 accepted contract。事务日志应链接回 RFC，RFC 也应链接到事务日志。若实现期反馈只改变执行事实，追加事务日志即可；若它改变阶段计划、验证 floor、不变量、ABI 或接受边界，必须同步更新对应 RFC 文本。

事务日志还应标明实现的 RFC 修订。首次实现通常对应 `R0`；Closed RFC 的后续 `R1`、`R2` 如果需要代码工作，应分别建立可独立收口的新事务。旧事务继续作为原修订的执行证据，不重新打开。

## Tracking Issues

不是每个 RFC 都需要 `tracking-issues.md`。只有当问题清单会影响实现顺序、review gate、停止边界或验收判断时，才在 RFC 根目录创建它。

`tracking-issues.md` 是当前问题跟踪页，不是历史归档：

- 当前仍影响实现或验收的问题放在 `tracking-issues.md`；
- 实现期暴露出的接口摩擦、状态机不闭合、边界错误或抽象过度，若会改变 accepted contract，也应作为设计问题进入 `tracking-issues.md`；
- 已过期的旧问题清单、被否决方案和历史 review 材料放在 `backgrounds/`；
- 实际阶段推进、checkpoint、验证证据和更正说明仍写入事务日志；
- 不要用它替代 GitHub issue、PR 讨论或双周 devlog。

问题等级必须使用当前 review skill 的名称：

- `Apollyon`：错误结果、数据损坏、安全问题、崩溃或严重不可恢复状态，必须修。
- `Keter`：不会马上爆炸，但会阻塞后续开发或把核心抽象带错方向，必须修。
- `Euclid`：通常值得修，但不阻塞主线。
- `Safe`：记录即可，默认不修，除非局部且低成本。
- `Neutralized`：已经处理完成的问题；必须保留 neutralize 依据和对应事务日志条目。

旧文档可能仍出现 `P0/P1/P2/P3` 历史称呼；新增 RFC、review 输出和 tracking issue 不再使用这些旧等级名。

## 当前 RFC

- [RFC-20260629-vfs-direct-user-io](./rfcs/vfs-direct-user-io/index.md)：已实现第一版；定义普通文件 `read` / `readv` / `pread*` 与 `write` / `writev` / `pwrite*` 的 direct userspace copy 边界、VFS-owned user-buffer cursor、fanotify transaction adapter，以及 ramfs/ext4 regular file read/write hook。`RWF_*`、完整 Linux `O_DIRECT`、mmap coherency、splice family 和 non-regular backend hook 仍按 register / follow-up 边界处理。
- [RFC-20260620-threaded-timer-event](./rfcs/threaded-timer-event/index.md)：已实现第一版；定义 soft timer 的 threaded completion lane、per-CPU timer worker、通用 `Late` initcall、`timerfd` / `ITIMER_REAL` 迁移边界，以及 wait-core timeout 非目标。
- [RFC-20260618-sched-wait-preempt-arming](./rfcs/sched-wait-preempt-arming/index.md)：阶段 3 已关闭；定义 wait-core 在 kernel preempt 下的 wake-prerequisite / parkability contract、scheduler entry split、preempt-defer、token-bound wait sleep、single-active-wait 诊断和 feedback routing 边界；未运行的 trace / fairness evidence gap 见事务日志。
- [RFC-20260711-sched-rt-class](./rfcs/sched-rt-class/index.md)：R0 已完成共享 `Realtime` class、FIFO/RR policy、typed priority、99 个 priority bucket 与 RR quantum；R1 已由 `39ba07a9` 完成并关闭，删除 class-visible resched cause continuation，以 RR-owned `rotation_due` 表达 committed rotation，并把 pending 收窄为 processor-owned single bit。R0 证据见 [2026-07-12-sched-rt-class](./devlog/transactions/2026-07-12-sched-rt-class.md)，R1 证据见 [2026-07-14-sched-rt-class-r1](./devlog/transactions/2026-07-14-sched-rt-class-r1.md)。
- [RFC-20260713-sched-fair-stride](./rfcs/sched-fair-stride/index.md)：已完成第一版；稳定 `Fair` class identity、经典 fixed-tick Stride、Linux nice weight、最小 pass heap、placement floor、yield handoff与 compile-time default selector已经落地。用户完成 Fair default `all` LTP profile和 `fair-test`，并接受相对 RT/RR约 13–14%的同量级耗时差距；IRQ-off heap allocation继续由 register跟踪。事务证据见 [2026-07-13-sched-fair-stride](./devlog/transactions/2026-07-13-sched-fair-stride.md)。
- [RFC-20260714-sched-dynamic-attributes](./rfcs/sched-dynamic-attributes/index.md)：公开 Draft；提议以 scheduler-owned config patch 与固定 owner-CPU `RunQueue` transaction 统一动态 nice、Fair/RT policy、RT priority、reset-on-fork 与 fixed-CPU affinity，并以 async IPI、parked one-shot completion 和临时全局 remote submission gate保持同步 syscall 语义。R0 尚未接受，implementation transaction 尚未建立。
- [RFC-20260616-kthread-core](./rfcs/kthread-core/index.md)：已接受、阶段 6 implementation gate 已关闭；纠偏 kthread core，定义 procfs-visible singleton thread group、固定 `kthreadd` TID 2、strong handle、专用 exit、user-facing API fail-closed，以及移除 service/park 的迁移 gate。
- [RFC-20260614-kthread](./rfcs/kthread/index.md)：历史基线；记录已落地的轻量 kthread 创建代理、typed entry、stop/park 生命周期和 `KThreadService` 后台 worker 合同，已由 `kthread-core` supersede。
- [RFC-20260614-inode-shrinker](./rfcs/inode-shrinker/index.md)：自循环 `io_shrink_threshold` gate 的 inode cache shrinker、superblock eviction path 和 ext4 backing file cache 计数合同。
- [RFC-20260615-oom-killer](./rfcs/oom-killer/index.md)：物理页阈值触发的 OOM killer、按独占物理页选择用户进程和 clone 内存压力 user-app-test 计划。
- [RFC-20260602-cred-merge](./rfcs/cred-merge/index.md)：credentials feature merge 的 canonical 执行计划和审查合同。
- [RFC-20260606-signal-temp-mask-restore](./rfcs/signal-temp-mask-restore/index.md)：`rt_sigsuspend`、`ppoll`、`pselect6` 临时 signal mask delayed restore 协议、trap-return delivery handoff 和 staged 实施计划。
- [RFC-20260605-fileops-seek-char-ioctl](./rfcs/fileops-seek-char-ioctl/index.md)：`FileOps::seek`、positioned I/O 分层和字符设备 ioctl 默认分发计划。
- [RFC-20260603-IOCTL-LOOP](./rfcs/ioctl-loop/index.md)：`ioctl(2)` VFS 分发、通用块设备 ioctl 和 loop 设备最小闭环计划。
- [RFC-20260604-fanotify](./rfcs/fanotify/index.md)：fanotify path-fd 通知、group fd、mark registry 和 staged LTP 兼容计划。
- [RFC-20260604-mount-tree-legacy-api](./rfcs/mount-tree-legacy-api/index.md)：第一版已实现并完成阶段 7 收口；保留 shared/slave/unbindable propagation、mount flag matrix、fstype alias bridge、ROFS mmap/writeback 和 unmount cleanup 等 register limitations。
- [RFC-20260604-proc-tgid-fd](./rfcs/proc-tgid-fd/index.md)：`/proc/<tgid>/fd` 目录枚举、fd symlink `readlink()` 和第一阶段 procfs/fd 兼容计划。
- [RFC-20260603-sched-latch](./rfcs/sched-latch/index.md)：`poll` / `select` OR wait 所需的 wait-core latch 原语和 iomux 迁移计划。
- [RFC-20260601-sched-wait-refactor](./rfcs/sched-wait-refactor/index.md)：R0 已完成；post-close tracker 记录 synchronous remote placement 与 cross-CPU IPI completion 的组合问题。

## 已关闭或延期 RFC

- [RFC-20260622-sched-eevdf-lite](./rfcs/sched-eevdf-lite/index.md)：Stage 3/R1 runtime acceptance 失败后延期关闭，不是 Completed；关闭时 default 曾恢复为 RR，后续已由 Fair / Stride 切换为 Fair。EEVDF 保留为可运行实验原型，但显著吞吐回归与百万级 yield self-pick 仍存在，`EEVDF-001` / `EEVDF-018` / `EEVDF-004` / `EEVDF-020` 保持未解决 Keter。事务日志见 [2026-07-09-sched-eevdf-lite](./devlog/transactions/2026-07-09-sched-eevdf-lite.md)，证据见 [Stage 3 eligibility 回归背景](./rfcs/sched-eevdf-lite/backgrounds/stage3-eligibility-regression-20260711.md)。

当一个 feature 被多个 RFC 分段覆盖时，本页可以作为轻量聚合入口，或由其中一个 umbrella RFC 在 `index.md` 中聚合链接。聚合入口只列出相关 RFC、事务日志、register / current limitations 及其覆盖范围；不要在这里复制阶段完成度、验证证据或问题状态。跨 RFC 功能的当前事实仍以各自 RFC、transaction devlog 和 register / current limitations 为准。

## 目录级 RFC 何时必需

满足以下任一条件时，必须使用目录级 RFC，而不是单文件草案：

- 迁移跨多个子系统，且需要阶段性实施计划；
- 方案正确性依赖明确不变量或协议证明；
- 需要保留历史备选、问题清单、review 结论或验证证据；
- devlog 事务日志需要引用该计划作为 canonical source。

## 如何避免误导 agent

只要边界清楚，公开草案不会误导 agent。

关键在于：

- 当前事实仍写在主文档、活动记录和已接受的决策记录中；
- 草案只陈述提议、问题与待决事项；
- 草案一旦被接受，其结论应迁移到当前事实页面、决策记录，或在 RFC 目录内标记为 canonical implementation source。

换句话说，草案是输入材料，不是当前事实本身。
