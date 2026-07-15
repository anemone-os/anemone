# Dynamic Scheduler Attributes Tracking Issues

**状态：** Active
**最后更新：** 2026-07-15
**父 RFC：** [RFC-20260714-sched-dynamic-attributes](./index.md)
**事务日志：** None

本文只跟踪 confirmed design issue。implementation checkpoint、write set或验证尚未执行本身不作为design issue；只有阶段边界暴露出错误owner、contract、停止条件或无法完成的验证义务时才进入本页。accepted contract 必须修回 [RFC 入口](./index.md)、[不变量需求](./invariants.md) 或[迁移实施计划](./implementation.md)；本文不替代canonical正文。

## Apollyon

- 暂无。

## Keter

- 暂无。

## Euclid

- 暂无。

## Safe

- 暂无。

## Neutralized

### KETER-DYNATTR-005：SMP 与 targeted LTP 验证无法在声明的 write set 内执行

**状态：** Neutralized in implementation plan / 2026-07-15 document-layer review

**原问题：** Phase 3要求rv64 SMP=2，但live pretest platform固定`smp = 1`且QEMU xtask没有runtime override；Phase 3至5又要求targeted schedule LTP，而active profile通过tracked`profile.txt` compile-time选择。原write set没有包含这两个真实validation owner，worker只能越界临时修改或把整套`all`误写成targeted证据。

**修复落点：** [迁移实施计划](./implementation.md)把`conf/platforms/qemu-virt-rv64-pretest.toml`与`anemone-apps/user-test/ltp/profile.txt`列为validation-only write set，要求runtime后恢复并验证无diff；P2 minimum write set只保留rv64 pretest rootfs，`minimal`与la64 wiring留到阶段6确认长期资产时决定。

**关闭理由：** 验证现在有真实owner、恢复条件与证据边界，不需要扩大QEMU CLI或建立第二套测试平台；validation-only输入不得进入最终提交。

### KETER-DYNATTR-004：Phase 3 无法决定性命中 Force / late-completion race

**状态：** Neutralized in implementation plan / 2026-07-15 document-layer review

**原问题：** 原P2把user-space Force winner与closed-receiver late completion列为SMP runtime必过项，但focused app无法稳定把IPI handler停在request已发布、receiver已注册而completion尚未发送的窗口；同时计划禁止test-only transport/delay。该gate可能偶然通过却没有命中所需竞争，无法形成可重复证据。

**修复落点：** Phase 1决定性one-shot KUnit证明force winner、receiver close、trigger detach、late send与payload exactly-once；Phase 2B request tests证明request exactly-once；Phase 3 source review证明`close receiver -> detach trigger -> finish Latch -> release guard`顺序。P2 runtime只保留SMP=2 mutual setter、gate contention、request count assertion、read-back与正常shutdown。

**关闭理由：** Force合同和late-send不变量没有削弱，只把不可控的user-space race拆成决定性module test与owner-local source proof；Phase 3不再要求flaky Force smoke。

### KETER-DYNATTR-003：Phase 2 原子 checkpoint 过大，P1 无法隔离风险

**状态：** Neutralized in implementation plan / 2026-07-15 document-layer review

**原问题：** 原Phase 2把priority目录搬迁、IPI `Copy -> Clone`、typed config、class transition、storage cutover、request transport、clone与procfs放进一个minimum write set。P1只有在完整cutover后才能review，失败时无法区分mechanical regression、typed model错误或owner transaction问题。

**修复落点：** Phase 2拆为2A与2B。2A只允许behavior-preserving priority move、IPI clone mechanical变化和不安装storage/不发布production path的typed foundation；2B才原子安装唯一`SchedConfig`、删除`AtomicNice`与direct setter，并同时切换owner transaction、remote request、priority、clone和procfs。P1 minimum write set改为2B，2A有独立build、KUnit、source audit与review gate。

**关闭理由：** 单一truth约束仍完整落在2B，mechanical/dormant preparation不再和semantic cutover混审；2A保留的existing weak setter有紧邻2B的明确退出条件，不能演化为长期adapter。

### KETER-DYNATTR-002：调度 UAPI 的精确 ABI matrix 尚未闭合

**状态：** Neutralized in RFC Draft contract / 2026-07-15 document-layer review

**原问题：** RFC Draft 已确定 syscall 集合、supported policy 与内部 patch，但没有逐项固定 `sched_attr` size negotiation/zero tail、supported flags、legacy `SCHED_RESET_ON_FORK` encoding、unknown/unsupported flag、pid/usize corner case、copy-in/copy-out ordering、errno precedence和Fair/FIFO/RR interval。继续实现会把 ABI 决策散落到 handler，并可能为了单个LTP branch临时改变errno。

**修复落点：**

- [Linux 6.6 Scheduler UAPI Matrix](./backgrounds/linux-6.6-sched-uapi-matrix.md)记录Linux 6.6 layout、逐syscall ordering、permission/errno、interval和LTP/POSIX evidence。
- [RFC入口](./index.md)固定known size 56、VER0/VER1 negotiation、reset-only attr flags、legacy reset encoding、field-to-patch mapping、getter projection、permission matrix和interval table。
- [不变量需求](./invariants.md)固定raw UAPI containment、setter/getter/affinity failure phase、typed permission denial和禁止stale complete-config模拟。

**接受取舍：** R0只支持`SCHED_FLAG_RESET_ON_FORK`。KEEP_POLICY、KEEP_PARAMS、reclaim、deadline overrun、util clamp和unknown flags返回`EINVAL`；BATCH、IDLE与DEADLINE setter继续是明确非目标。拒绝KEEP_PARAMS避免为未要求feature新增owner-side patch，也避免syscall snapshot read-modify-write。`sched_attr`仍advertise Linux 6.6 size 56，feature支持不通过缩短struct伪装。

**ABI结论：** raw structs/flags只在`sched/api`；setter最终仍只生成既有semantic patch；permission denial是typed internal result，由`setpriority()`映射`EACCES`、其它scheduler setter映射`EPERM`；getter只从一个`SchedConfig` snapshot投影。Fair interval为一个effective tick，FIFO为zero，RR为full configured effective quantum。

**验证边界：** stock LTP的`sched_setattr01`/`sched_getattr01` Deadline success和`sched_setscheduler03` BATCH/IDLE success与R0非目标冲突，已分类为expected unsupported coverage，不能作为整case completion gate。supported branches与size/zero-tail、cross-error precedence、raw affinity length和Fair interval由focused syscall tests在未来implementation plan中列gate。

**关闭理由：** 文档修复没有要求新增configured field、permission state、transaction owner或新的core patch维度；Keter要求的field、flag、errno、copy ordering和observable interval均已有canonical proposal与source evidence，R0 acceptance不再被该问题阻塞。

### KETER-DYNATTR-001：cross-CPU one-shot completion 不能复用 synchronous remote wake

**状态：** Neutralized in this RFC / Root issue transferred to wait core

**原问题：** live `LatchTrigger::trigger()` 通过 wait core 立即执行 physical placement；receiver 位于其它 CPU 时，当前 `remote_wake_enqueue()` 使用 synchronous IPI 等待。若 CPU A 与 CPU B 的 IPI handler 同时完成对方发起的 scheduler request，双方可能在 handler 内互等反向 wake IPI。

**责任归属：** 通用缺口由公开 wait-core tracker 的 [KETER-WAIT-001](../sched-wait-refactor/tracking-issues.md#keter-wait-001synchronous-remote-placement-不能组合进-cross-cpu-ipi-completion) 负责。本 RFC 不增加 reverse-completion protocol、pre-reserved message、deferred worker、第二 mailbox或allocation-free IPI transport。现有IPI allocation失败继续服从内核fatal OOM接受边界，不作为本RFC的可恢复transaction error。

**本 RFC neutralize：** `sched/api` 使用一个 module-private、全局、sleepable `Mutex<()>` `REMOTE_SCHED_REQUEST_GATE`。现有Mutex已经是CAS fast path加内部Event slow path，本RFC不另建`AtomicBool + Event` gate。每个remote request在发布前获取guard，并持有到`recv_uninterruptible()`返回，使全系统最多一个published request仍有开放receiver、其completion仍可能触发wait-core placement；因此不可能出现两个scheduler request handler同时完成对方wait。dormant channel构造不建立wait，可以发生在gate前或gate后；gate不得在receive已经建立active wait后获取。guard刻意跨越receive的Latch park持有，但receive内不再获取sleepable lock，gate也不进入IPI handler、config或RunQueue transaction。one-shot选择Latch而非Event，因为当前`Event::listen_uninterruptible()`吞掉Force并重试predicate，无法表达已接受的`RecvError::Forced`，且Event的reusable listener/quota policy不属于该single-consumer channel。

**Force边界：** Force winner必须先关闭receiver、detach trigger并finish Latch，再返回和释放gate。旧request envelope此时仍可能留在IPI queue或handler中，但late send只能观察receiver closed，不能再触发remote placement；因此neutralization不声称所有时序下物理上最多一个request envelope in flight。

**Exactly-once repair：** `Arc<SchedRequest>` 内只有一个 `NoIrqSpinLock<Option<SchedRequestBody>>`；body拥有target、patch、permit和non-clone sender。handler在transaction前take body，第二次execute或double-complete由常开断言暴露，Arc clone不能复制execution capability。

**验证与退出：** Phase 1决定性one-shot KUnit覆盖send-before-receive、send-between-latch-begin-and-trigger-registration、send-after-registration、sender/receiver提前drop、force winner、closed-receiver late send与payload exactly-once drop；Phase 2B focused tests覆盖request exactly-once，request/guard source review证明Force返回前已经close receiver、detach trigger并finish Latch。Phase 3 runtime只要求双CPU并发互调setter、`Mutex<()>` gate contention、receive前transport failure关闭/丢弃dormant endpoints后再release gate、request/read-back一致与正常shutdown，不要求时序不可控的user-space Force smoke。测试断言的是不会同时存在两个仍有开放receiver且可触发placement的published request，不是request envelope总数。wait-core接受hardirq-safe remote placement并完成实现后，移除gate并复跑同一双向stress；在此之前不得把KETER-WAIT-001标记为已修复。
