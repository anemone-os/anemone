# TTY Subsystem Tracking Issues

**状态：** Closed（0 个 Keter 开放，12 个 Keter 已中和）
**最后更新：** 2026-07-23
**父 RFC：** [RFC-20260722-tty-subsystem](./index.md)
**事务日志：** [2026-07-23 - TTY Subsystem](../../devlog/transactions/2026-07-23-tty-subsystem.md)

本文只跟踪已经确认、会影响 target、状态所有权、ABI 边界、实现顺序、review gate、停止边界或验收判断的 design issue。普通 TODO、尚未冻结的工程参数和背景材料中已经被正文吸收的现状缺口不在这里重复登记。

当前没有开放 blocker。Stage 2 closure review为0 Apollyon / 0 Keter / 0 Euclid，`TTY-DATA-CUTOVER`已经
Effective；该结果只关闭data-plane proof，不改变下列future relation/job-control finding的重新打开条件。
十二个Keter均保留在Neutralized区，记录原问题、修复位置、首版接受边界和重新打开条件；实现证据若突破这些
条件，必须重新进入文档review，不能只在implementation中改变语义。

## Apollyon

None.

## Keter

None.

## Euclid

None.

## Safe

None.

## Neutralized

### KETER-012 — C4 双架构 proof 与明确的 RV64-only validation disposition 冲突

**状态：** Neutralized（2026-07-23）

**原问题：** R0 的 Stage 2 C4 把 RV64 与 LA64 自动 matrix 同时列为 `TTY-DATA-CUTOVER` 必要证据，但本轮用户明确不运行 LA64 build/runtime，并要求后续同类 checkpoint 采用相同处置。若静默把 LA64 写成 Not Run 后仍按 R0 cut over，会让 accepted proof boundary 与 transaction evidence 冲突；若为满足文档强行运行 LA64，又违反明确 validation disposition。

**决策：** 形成 R1 accepted revision。TTY 功能目标、correctness invariant、owner、ABI、visible semantics、两个 cutover unit与完整case matrix全部保持不变；只把本 RFC 的 build/runtime acceptance architecture 收窄为 RV64。C4只交付RV64 rootfs/wrapper并以用户指定的初赛musl BusyBox完成RV64自动matrix和人工vi checklist；LA64 compile/runtime明确为Not Run，不阻塞cutover，也不得由RV64证据外推。source/KUnit、agent-run与user-run证据仍必须分层，不能借架构处置删除语义case。

**修复位置：** [接受边界](./index.md#接受边界)、[Prospective cutover 边界](./invariants.md#prospective-cutover-边界)、[`TTY-LOCAL-005`](./invariants.md#tty-local-005--验收证据必须分层且诚实)、[Stage 2 C4](./implementation.md#checkpoint-4--userspace-acceptance-与-tty-data-cutover)和[Gate P3](./implementation.md#gate-p3--busybox-ashvi-acceptance)。

**重新打开条件：** 后续目标或consumer要求LA64成为发布承诺、出现LoongArch专属TTY实现差异、RV64-only proof被误写成LA64兼容结论，或准备恢复多架构cutover floor。

### KETER-010 — Signal direct dependencies 未进入 Contract Impact

**状态：** Neutralized（2026-07-22）

**原问题：** TTY terminal access需要读取current mask/live disposition、生成ordinary occurrence并依赖DefaultStop action selection，但prospective `Contract Impact`只Preserve了process-group selector、jobctl、lifecycle与user-entry，没有登记Signal pending/action owner。Stage 3/4因而可以声称保护“全部Preserve IDs”，同时遗漏直接参与handoff的Signal contract。

**决策：** 最小contract闭包增加`SIGNAL-PENDING-001/002`和`SIGNAL-ACTION-001/002`为Preserve。TTY只请求Signal owner完成blocked/ignored/actionable decision与ordinary occurrence publication，不保存pending、不自行提交ignore/handler/default-stop，也不让notification或signal结果反向驱动relation。`SIGNAL-TEMP-MASK-*`没有直接target delta，本次不扩大闭包。

**修复位置：** [`Contract Impact`](./invariants.md#contract-impact)、[`TTY-JOBCTL-001`](./invariants.md#tty-jobctl-001--terminal-policy-只产生经重验的-guards-out-effect)、[Stage 3 Ready](./implementation.md#8-stage-3-readycontrolling-relation-与-callertopology-vertical-slice)和[Stage 4 Outline](./implementation.md#9-stage-4-outlineterminal-job-control完整验收与-tty-jobctl-cutover)。

**重新打开条件：** implementation绕过Signal owner读取或缓存mask/disposition/pending，新增handoff改变temporary-mask responsibility，或terminal signal不再通过现有pending/action/jobctl路径闭合。

### KETER-011 — Relation cleanup 的 `SIGHUP`/`SIGCONT` acceptance 不明确

**状态：** Neutralized（2026-07-22）

**原问题：** 正文曾把“当前测试入口能够覆盖的`SIGHUP`/`SIGCONT` effect”列入首版包络，但`TTY-ABI-001`和Stage 4只要求模糊的relation cleanup。这会让`TTY-JOBCTL-CUTOVER`既可以按“只撤销relation”关闭，也可以被解释为必须向旧foreground process group生成signals，accepted target和验证边界不唯一。

**决策：** 首版session-leader `TIOCNOTTY`和exit cleanup只撤销controlling relation与foreground selector，使旧relation不能再被`/dev/tty`、access check或mutation取得；不生成relation-disassociation `SIGHUP`/`SIGCONT`。这两个effects作为scoped limitation留在首版target之外，并与hardware hangup及newly orphaned stopped process-group effects分开记录。后续加入时必须通过target review定义触发条件、旧foreground snapshot、Signal handoff、oracle和register disposition，implementation stage无权自行扩展。

**修复位置：** [目标与非目标](./index.md#目标)、[Controlling terminal 与 job-control handoff](./index.md#8-controlling-terminal-与-job-control-handoff)、[Lifecycle cleanup](./index.md#9-lifecyclerelation-cleanup-与延后的-hangup)、[`TTY-LIFE-001`](./invariants.md#tty-life-001--relation-cleanup-先撤销可发现性再执行外部效果)、[`TTY-ABI-001`](./invariants.md#tty-abi-001--首版兼容包络必须真实可观察)、[Stage 3](./implementation.md#8-stage-3-readycontrolling-relation-与-callertopology-vertical-slice)和[Stage 4](./implementation.md#9-stage-4-outlineterminal-job-control完整验收与-tty-jobctl-cutover)。

**重新打开条件：** 新增consumer或oracle要求detach/exit signal effects，implementation需要依赖旧foreground snapshot生成signal，或register/验收试图把relation撤销证明写成`SIGHUP`/`SIGCONT`能力。

### KETER-002 — background access current-caller handoff 与 ash `SIGTTOU` 包络

**状态：** Neutralized（2026-07-22）

**原问题：** background read、`TOSTOP` write 和 terminal-modifying ioctl 必须按每次操作的 current caller 判断 controlling relation 与 foreground membership，不能使用 opener identity；通用 `FileIoCtx` / `IoctlCtx` 又不应为了 TTY 携带完整 task topology 和 Signal 状态。初次修复把 blocked/ignored `SIGTTOU` 与全部 terminal-modifying ioctl 一并延期，随后复审确认这与 ash `fg` 目标冲突：ash 会忽略 `SIGTTOU`，并在 foreground job 结束或停止后从 background 通过 `TIOCSPGRP` 把 terminal 收回给 shell；job-side `tcsetpgrp()` race 也可能依赖继承的 ignore disposition。

**决策：** 每次 TTY read/write/ioctl 在同步 `FileOps` entry 从 current task 构造短命 caller capability/snapshot；TTY 只在 relation owner 内取得 controlling/foreground snapshot，随后 guards-out 调用 task topology/Signal 的窄 decision API 做 caller membership、signal mask/disposition 与 target authority revalidation，并返回 continue、signal-and-restart、`EIO`/unsupported 或 retry 一类 owner-local 结果。完整 `Task`、opener identity、topology lock 和 signal state 不得存入 `Terminal`。

首版必测包络包括普通 background read + default-action `SIGTTIN`，以及 `TIOCSPGRP` 的三条非 orphan 路径：foreground caller 验证后允许；background caller 当前 `SIGTTOU` blocked 或 ignored 时允许；background caller 的 `SIGTTOU` 可生效时，向 caller process group 生成 signal 并以可重启 syscall 结束，foreground mutation 不得提前提交。relation mutation 必须在 decision 后返回 relation owner 重验 generation 再 commit。普通 background read 的 blocked/ignored/orphan 分支、`TIOCSPGRP` orphaned-pgrp errno、`TOSTOP` write 与其它 terminal-modifying ioctl corner 继续等待各自定向 oracle，可以 fail-close 或登记 scoped limitation；它们不得反向覆盖 ash 必经的 `TIOCSPGRP` 路径。

**修复位置：** [首个交付边界](./index.md#1-首个交付边界分成-checkpoint-与完成目标)、[Controlling terminal 与 job-control handoff](./index.md#8-controlling-terminal-与-job-control-handoff)、[接受边界](./index.md#接受边界)和[`TTY-ABI-001`](./invariants.md#tty-abi-001--首版兼容包络必须真实可观察)。

**重新打开条件：** live `FileOps` 不能稳定取得 current caller、窄 decision API 无法在不下沉完整 `Task` 的情况下区分上述三条 `TIOCSPGRP` 路径、relation mutation 无法在 guards-out decision 后安全重验并 commit，或新增验证把其它延后 background-access corner 纳入首版兼容包络。

### KETER-003 — relation cleanup 混合了不同 owner 的状态转换

**状态：** Neutralized（2026-07-22）

**原问题：** session-leader detach、backend hangup、foreground process group 消失和 newly orphaned stopped process group 被混写为 relation cleanup，但它们不终结同一份状态，可能让 TTY 错拆 session binding 或成为 orphan policy 的第二 owner。

**决策：** 首版只有 session leader detach/exit 终结 controlling relation，由 TTY relation owner cleanup，且只撤销relation/foreground selector，不生成relation-disassociation `SIGHUP`/`SIGCONT`；foreground group 消失只使 foreground selector 失效，不拆 relation。orphan transition、判定及其`SIGHUP`/`SIGCONT` effect唯一属于task topology/jobctl，即使首版因缺少oracle延后该effect，也不改变owner。foreground cleanup 可以使用窄通知或下一次 relation revalidation 惰性收敛。hardware hangup/backend fatal failure 的检测、relation effect、fd/open ABI 与 deferred-consumer quiesce 均延后；它们后续进入兼容包络时，hangup truth 和 relation cleanup 仍只能由 TTY owner 持有，且不得通过 runtime unpublish、编号回收或销毁 `Terminal` 代替 ABI。

**修复位置：** [Lifecycle、relation cleanup 与延后的 hangup](./index.md#9-lifecyclerelation-cleanup-与延后的-hangup)、[Controlling terminal 与 job-control handoff](./index.md#8-controlling-terminal-与-job-control-handoff)和[接受边界](./index.md#接受边界)。

**重新打开条件：** implementation 需要由 TTY relation 驱动 orphan policy、foreground selector 失效被证明必须拆除 controlling relation，新增测试要求更强的跨 owner 原子 cleanup，或 hardware hangup/backend failure 取得可执行 oracle 并准备进入兼容包络。

### KETER-004 — termios truth 与 compatibility policy 尚未闭合

**状态：** Neutralized（2026-07-22）

**原问题：** committed termios 是用户可见 truth，但草案没有说明 boot-applied baud/data/parity 如何形成初始 snapshot，也没有区分必须执行的 semantic flags、行为等价 compatibility bits 和真实 invalid combinations。

**决策：** 初始 speed 与 hardware-backed `c_cflag` 从 boot-applied UART line configuration 派生，其余初始 semantic flags 由 TTY default policy 给出。首版分三类处理：已承诺 semantic flags 真实执行；行为等价 compatibility bits 稳定 round-trip、归一化或静默忽略，并对非默认请求保留限频诊断；真实无效、无法表示、会伪造硬件能力或必须实际改变 baud/data bits/parity/stop bits 的组合稳定失败且不发布新 snapshot。运行时硬件线路重配置不属于首版完成条件；只有后续 oracle 将具体字段纳入兼容包络时，才建立 backend apply/rollback 协议。精确 bit mask/errno 只在有可执行用例的 implementation gate 冻结，不为无法验证的 Linux 全矩阵提前建机制。

**修复位置：** [`TtyPort` 物理端口能力](./index.md#ttyport物理端口能力)、[第一版 termios 与 ioctl 下限](./index.md#6-第一版-termios-与-ioctl-下限)和[兼容与工程原则](./index.md#兼容与工程原则)。

**重新打开条件：** BusyBox `stty`/`ash`/vi 的读改写证明三类 policy 不足、boot configuration 不能形成可信初始 snapshot，或新增 workload/oracle 需要把目前行为等价或被拒绝的字段升级为真实 runtime hardware semantics。

### KETER-001 — SID-only relation 无法表达完整 `TIOCNOTTY` 语义

**状态：** Neutralized（2026-07-22）

**原问题：** `/dev/tty` 仅按 caller session identity 解析 controlling relation；若同时承诺 Linux 的 non-session-leader `TIOCNOTTY`，caller 局部 detach 后再次 open 会通过同一 SID 重新取得 terminal，除非再增加 per-ThreadGroup attachment truth/capability。

**决策：** 第一版放松 ABI 约束，不增加 per-ThreadGroup attachment。只允许 session leader 的 `TIOCNOTTY` 拆除整个 relation；同 session non-leader 调用返回 `EPERM` 并记录 scoped-deviation 诊断。`TIOCSCTTY` 只支持 `arg=0` 普通 acquisition；`arg=1` privileged steal/rebinding 返回 `EPERM`。session-wide relation 继续是唯一 controlling-terminal truth。

**修复位置：** [controlling-terminal relation](./index.md#controlling-terminal-relationsession-与-terminal-的单一绑定)、[VFS、devfs 与 boot 接线](./index.md#7-vfsdevfs-与-boot-接线)、[Controlling terminal 与 job-control handoff](./index.md#8-controlling-terminal-与-job-control-handoff)和[接受边界](./index.md#接受边界)。

**重新打开条件：** userspace 验证或后续 RFC 明确要求 non-leader detach、late per-ThreadGroup attachment 或 privileged steal；届时必须设计不复制 session-terminal relation truth 的窄 attachment/capability，并重新审查 fork/exec/setsid/session cleanup 与 `/dev/tty` lookup。

### KETER-005 — 首个 `/dev/ttyS0` 验收实例被误读为 registry 单例

**状态：** Neutralized（2026-07-22）

**原问题：** 正文只反复点名 `/dev/ttyS0`，又没有明确区分静态设备号分区、启动期 endpoint discovery 和 publish 后节点生命周期，容易把首个必交付实例误实现成 singleton registry，或者反过来为多 port 提前引入 runtime hotplug/unpublish。

**决策：** 注册单位是一个选择 TTY frontend 的物理 serial port，而不是整个 controller；每个启动期成功注册的 `TtyPort` 分别形成一个共享 `Terminal` 和 `/dev/ttyS<N>`。TTY owner 从 immutable port identity 经固定平台表、firmware alias 或确定性 allocator 生成稳定 `N`，不能依赖并发 probe 的偶然完成顺序。create transaction 在 devfs publish 前完成 terminal、handoff 与所选 deferred carrier 的 consumer binding；publish 前失败可以回滚，publish 成功后 node、编号和 terminal object 保持到重启，第一版不支持 runtime hotplug、unpublish、重新编号或编号复用。`/dev/ttyS0` 只是首个必交付实例。

**修复位置：** [摘要](./index.md#摘要)、[目标与非目标](./index.md#目标)、[`TtyPort` 物理端口能力](./index.md#ttyport物理端口能力)、[VFS、devfs 与 boot 接线](./index.md#7-vfsdevfs-与-boot-接线)、[Lifecycle、relation cleanup 与延后的 hangup](./index.md#9-lifecyclerelation-cleanup-与延后的-hangup)和[接受边界](./index.md#接受边界)。

**重新打开条件：** 平台需要 boot 完成后的 serial hotplug/remove、已发布 endpoint 的重新编号/复用，现有固件 identity 无法形成稳定 name，或一个物理 port 需要同时承载多个独立 serial terminal endpoint。

### KETER-006 — `/dev/console` 发布责任错误地下沉到 TTY

**状态：** Neutralized（2026-07-22）

**原问题：** 正文让 TTY core 发布并解析 `/dev/console`，会使 TTY 接管 console registry、early/normal backend 切换和 selected-console policy，或者迫使 console 复制 terminal truth。同一 UART 可以同时投影 console backend 与 `TtyPort` capability，但这不构成 console 从属于 TTY 的 owner 关系。

**决策：** console 子系统唯一拥有 console registry、printk fan-out、early/normal backend 选择、selected-console truth 和永久 `/dev/console` node；TTY 只拥有 `/dev/ttyS<N>`、`/dev/tty` 和 terminal protocol state。首版不要求 `/dev/console` open 返回共享 `Terminal` file，也不预建 console-to-TTY open delegation；boot fd 选择可以消费 console owner 的 immutable selected-terminal identity，由 TTY revalidate 后安装真实 terminal file，但不转移任何 owner。若后续真实 consumer 需要 `/dev/console` reopen 共享 TTY 语义，再设计同样基于 immutable endpoint identity 的窄委托；early/output-only console 始终不得伪装成 TTY。第一版 NS16550A 从同一 physical owner 固定同时提供 output-only console backend 与 `TtyPort`，不再实现或注册 raw `CharDev`；TTY 独占 RX，console 不参与输入或运行时线路配置，boot-applied configuration 保持不可变，所有普通 console/TTY TX 经过 port-owned IRQ-safe serialization。两者不是互斥 personality，不建立 claim、lease 或 mode state；polling TX 使用有界 batch 或等价有界 queue，持有 serialization 的路径和 RX IRQ 均不得递归 printk。

**修复位置：** [背景 owner 结论](./index.md#背景)、[目标与非目标](./index.md#目标)、[输出、echo 与 console 共用端口](./index.md#5-输出echo-与-console-共用端口)、[VFS、devfs 与 boot 接线](./index.md#7-vfsdevfs-与-boot-接线)、[接受边界](./index.md#接受边界)和[风险](./index.md#风险)。

**重新打开条件：** console owner 无法暴露不泄漏 registry 内部状态的 selected-terminal capability，boot fd 接线无法在不复制 console/TTY selection truth 的情况下完成，真实 consumer/oracle 要求 `/dev/console` reopen 与选中 serial TTY 共享完整 terminal ABI，或后续引入 raw serial/serdev、console input、运行时线路重配置、hot-unplug/power-management，使固定共存关系必须升级为显式 claim/personality 协议。

### KETER-007 — user-copy fault 被过度提升为可回滚 input transaction

**状态：** Neutralized（2026-07-22）

**原问题：** 草案把 interrupted wait、普通 short read、mode transition 和 partial user copy 一并约束为“不丢失 input”，会要求 TTY 在释放 terminal guard 后仍为 copyout fault 保留可回滚队首 reservation，并进一步协调并发 reader、flush 和 mode transition。BusyBox `ash`/vi 的正常路径不依赖这种 fault replay，Linux 6.6.32 N_TTY 也会在 generic user copy 前推进 read tail；该承诺超出了首版兼容包络并会驱动不必要的跨层事务。

**决策：** 首版只保证有效缓冲区上的普通成功 read 不越过请求长度和 canonical record boundary，ordinary short read 未选入的 input 后缀继续排队；等待阶段在选择 input 前被 signal 中断时不消费。VFS 先校验 destination，TTY 可以在 terminal guard 内暂存有界 readable prefix并推进 queue，释放 guard后通过通用 read 路径 copyout。post-validation 地址失效或 partial copy 遵循通用 VFS fault/progress 语义，TTY 不回滚或 replay 已暂存但未复制的后缀，也不因这项 fault replay 单独要求或扩展 `read_user_transaction`。显式 flush、overflow 和 user-copy fault 是已记录的 input discard 边界，不得反向制造第二份 input truth。

**修复位置：** [兼容与工程原则](./index.md#兼容与工程原则)、[输入队列、read 与 readiness](./index.md#4-输入队列read-与-readiness)和[接受边界](./index.md#接受边界)。

**重新打开条件：** 新增可执行 TTY oracle 明确验证 copyout fault 后的 input replay，真实 shell/editor/runtime 依赖该行为，或通用 VFS read contract 改为对 backend consumption 提供可回滚事务；届时必须先比较 Linux ABI、generic read owner 与 TTY-local reservation 的边界，再决定是否扩大首版 target。

### KETER-008 — RX deferred mechanism 被过早冻结为专用 kthread 与 IRQ growth

**状态：** Neutralized（2026-07-22）

**原问题：** 正文把每 port 专用 RX kthread、`KThreadHandle::wake()` 和 hard-IRQ bounded fallible heap growth 写成首版必须保护的设计约束。这些选择可以形成合理实现，但不是用户可观察能力，也尚未由 live allocator、kthread lifecycle、内存预算或输入 burst 证据证明优于预分配 ring 与其它 deferred carrier；提前冻结会让 target 承担不必要的实现复杂度。

**决策：** target 只固定 hard IRQ 有界 drain、raw handoff 的单一 owner/顺序/overflow observability、notification-plus-predicate、deferred policy execution 与 no-lost-work。每 port 专用 RX kthread + `KThreadHandle::wake()` + bounded fallible IRQ growth 保留为明确候选；预分配 fixed ring 或满足同一 proof boundary 的其它现有 deferred facility 同样允许。implementation gate 根据 live 接口和证据选择最简单的路径；若选择候选方案，仍须证明 allocation failure、empty-to-nonempty notification、worker publication、drain budget 和 pre-publish rollback。

**Stage 0 implementation evidence（2026-07-23）：** live source确认预分配fixed ring可以让TTY-owned raw storage在IRQ中不分配；deferred carrier选用现有`KThreadHandle::wake()`。该API底层当前会进入register已记录的scheduler IRQ-off runqueue growth风险，但用户明确将其保留为wait-core/scheduler owner的既有问题：TTY不得因此停摆、发明workqueue/softirq或增加专用scheduler路径，也不得声称自己修复了该问题。TTY仍以ring predicate作为durable work truth，只在empty-to-nonempty边界调用窄wake，并保证自身IRQ/storage路径不新增allocation、复杂drop、普通日志或sleepable lock。

**修复位置：** [目标](./index.md#目标)、[RX、line discipline 与 deferred effects](./index.md#3-rxline-discipline-与-deferred-effects)、[并发与锁边界](./index.md#10-并发与锁边界)、[保留的工程余地](./index.md#保留的工程余地)和[迁移实施原则](./implementation.md#3-迁移原则)。

**重新打开条件：** TTY使用现有`KThreadHandle::wake()`仍无法保持所需predicate/owner boundary，必须由TTY新建deferred infrastructure或改变owner/lifecycle/failure semantics/acceptance boundary，或implementation probe发现预分配storage + kthread consumer无法满足no-lost-work与有界overflow。wait-core/scheduler自身已登记的IRQ-off placement风险不单独重新打开本条。

### KETER-009 — 首版提前承诺 runtime line configuration 与 backend hangup lifecycle

**状态：** Neutralized（2026-07-22）

**原问题：** BusyBox `ash`/vi 的当前包络只要求可信的 boot-applied termios snapshot、软件 line discipline 与 terminal job-control 主路径，但正文同时要求 runtime baud/data/parity/stop-bit apply/rollback，以及不可恢复 backend failure 后的 RX quiesce、relation cleanup、fd/open/poll 结果和持久 endpoint 行为。两组能力都缺少直接 consumer 和可执行 oracle，却会提前扩大 backend contract、状态机和 post-publish lifecycle。

**决策：** 首版从 boot-applied UART configuration 派生 hardware-backed termios truth；必须实际改变线路的请求可以稳定失败且不得发布新 snapshot，apply/rollback 只在后续具体字段进入兼容包络时建立。首版 relation cleanup 只覆盖 session leader detach/exit的relation撤销，不生成`SIGHUP`/`SIGCONT`；hardware hangup/backend fatal detection、hangup truth、relation effect、现有 fd/后续 open ABI 与 deferred-consumer quiesce 全部延后。后续扩展仍必须保持 UART hardware owner、TTY hangup/relation owner 和持久 devfs endpoint 边界，不能以 runtime unpublish 或第二份状态替代。

**修复位置：** [`TtyPort` 物理端口能力](./index.md#ttyport物理端口能力)、[第一版 termios 与 ioctl 下限](./index.md#6-第一版-termios-与-ioctl-下限)、[Lifecycle、relation cleanup 与延后的 hangup](./index.md#9-lifecyclerelation-cleanup-与延后的-hangup)、[接受边界](./index.md#接受边界)和[保留的工程余地](./index.md#保留的工程余地)。

**重新打开条件：** 新增 workload 需要 runtime hardware line change，hardware hangup/backend fatal failure 出现可执行 oracle，或 live backend 无法提供可信 boot-applied configuration snapshot；届时必须先扩展 target、测试和 backend/TTY handoff，再进入实现。
