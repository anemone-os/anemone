# 2026-07-23 - TTY Subsystem

**Status:** Active / Stage 0 Closed / Stage 1 Closed / Stage 2 Ready / Not Started
**Owners:** doruche, Codex
**Area:** device / TTY / serial / VFS / signal / task topology / job control
**Canonical Plan:** [RFC-20260722-tty-subsystem](../../rfcs/tty-subsystem/index.md), [目标与不变量](../../rfcs/tty-subsystem/invariants.md), [迁移实施计划](../../rfcs/tty-subsystem/implementation.md)
**Canonical Revision:** R0
**Current Phase:** Stage 2 Ready / Not Started / Checkpoint 1 Not Authorized

## Scope and contract boundary

本事务实现 R0 的滚动 stage。R0 接受 serial TTY owner、用户可见 ABI 包络、
`TTY-DATA-CUTOVER`、`TTY-JOBCTL-CUTOVER` 与 proof obligations，但不表示任何 TTY 能力已经
生效。Stage 0 只读审计 live VFS、UART、endpoint/boot、task topology、oracle 和模块边界；
它不修改 kernel、apps、rootfs、tests、register 或 current contracts。

Prospective `Introduce` IDs 为 `TTY-PORT-001`、`TTY-TERM-001`、`TTY-INPUT-001`、
`TTY-OUTPUT-001`、`TTY-ENDPOINT-001`、`TTY-REL-001`、`TTY-JOBCTL-001`、`TTY-LIFE-001`
和 `TTY-ABI-001`。全程 Preserve `SIGNAL-PENDING-001/002`、`SIGNAL-ACTION-001/002`、
`PGRP-SIGNAL-001/002`、`JOBCTL-STATE-001`、`JOBCTL-SIGNAL-001`、`JOBCTL-LIFE-001`、
`TASK-LIFE-001..003` 与 `USER-ENTRY-002`。

本次没有 contract cutover：`TTY-DATA-CUTOVER` 与 `TTY-JOBCTL-CUTOVER` 均为 Not Cut Over，
上述 `TTY-*` 均不是 current contract。

## R0 acceptance and Stage 0 activation

**Entry evidence:** 入口快照为 `dev/drc/omega@043f13c8`，worktree clean。文档层 R0 review
未发现 Apollyon、Keter 或 Euclid 级 target/owner/ABI/contract/acceptance blocker；当时
`tracking-issues.md` 中 11 个既有 Keter 均为 Neutralized。transaction 使用实际日期
`2026-07-23`，因此在 Stage 0 激活前将 implementation 中的精确路径从 2026-07-22 同步为
本文路径。用户已明确授权本轮建立 devlog 设施并完成 Stage 0 或触发停止合同。

**Frozen Stage 0 write set:** Stage 0 执行期只允许向本文追加 preflight、evidence matrix、
finding、review 与 closure/stop 事实。kernel、apps、rootfs、test profile、build config、current
contracts、register 与 RFC target 文本是只读输入。入口和状态导航由 RFC 工作流建立；它们不扩大
Stage 0 的 source write set。

**Activation:** R0、transaction、双向链接和导航建立后，Stage 0 进入 Active。以下证据全部来自
只读 source/config/oracle 审计；没有代码 instrumentation、probe app 或 runtime 执行。

## Stage 0 evidence matrices

### 1. VFS caller/open matrix

| 项目 | Live symbol / path | Owner / consumer | Failure signal | 候选路线 | 推荐路线 / 未决项 |
| --- | --- | --- | --- | --- | --- |
| Operation status | `FileIoCtx` / `WriteIoCtx` in `anemone-kernel/src/fs/file.rs`; `FileDesc` in `anemone-kernel/src/task/files.rs` | VFS/open-file-description owns live flags; TTY consumes per operation | TTY 缓存 `O_NONBLOCK` 会制造 stale per-file truth | 扩大 generic ctx；TTY entry 读取现有 ctx | 保持现有 ctx；TTY 每次从 open-file-description flags 读取，不缓存 |
| ioctl caller | `IoctlCtx` in `anemone-kernel/src/fs/file.rs`; syscall adapters in `anemone-kernel/src/fs/api/ioctl.rs` | VFS owns command/arg/access/userspace/fd lookup；task topology owns caller | 把完整 `Task`、fd table、topology/Signal state 下沉到所有 FileOps | 扩大 `IoctlCtx`；TTY entry 使用 `get_current_task()` | 采用 TTY 专用短命 caller capability；同步 entry 立即派生 stable identity，不存入 `Terminal` |
| devfs open | `DevfsNodeOps::open` in `anemone-kernel/src/fs/devfs/mod.rs`; leaf delegation in `anemone-kernel/src/fs/devfs/inode.rs` | devfs owns name/inode/dispatch；TTY owns caller-relative open policy | 用 generic `CharDev` 无法表达 `/dev/tty` caller-relative lookup | 修改 CharDev；TTY 专属 node ops | 使用 TTY 专属 open provider；`/dev/tty` 在同步 open entry 按 current session 查询 relation |
| Generic CharDev | `anemone-kernel/src/device/char/devfs.rs` | char registry only owns ordinary device dispatch | 给所有字符设备增加 task/session/poll surface | 通用 ctx 扩张；TTY sibling | 保持 CharDev 不变；TTY 是 `device::tty` sibling |
| read/copy fault | generic read path under `anemone-kernel/src/fs/api/read_write/` | VFS owns destination validation/copyout；TTY owns selected input prefix | 为 copyout fault 建立可回滚队首会制造第二份 input truth | reservation/replay；bounded kernel buffer | 先验证 destination，再由 TTY 暂存并推进 bounded prefix；post-validation fault 服从通用 VFS 语义 |
| poll/readiness | iomux request/register path under `anemone-kernel/src/fs/api/iomux/` | terminal predicate owns readability；latch/Event only notifies recheck | wake payload 或 hardware FIFO 成为第二份 readable truth；check/register 窗口 lost wake | 轮询；snapshot/register | source 在同一 guard 下 check predicate 并保存 latch trigger，`Armed` 只在该原子边界后返回 |

**VFS conclusion:** live VFS 能在不扩大 generic `CharDev`、不让 `Terminal` 保存完整 `Task` 的情况下
提供短命 caller handoff。此矩阵未命中 Stage 0 caller stop condition。

### 2. UART / transport matrix

| 项目 | Live symbol / path | Owner / consumer | Failure signal | 候选路线 | 推荐路线 / 未决项 |
| --- | --- | --- | --- | --- | --- |
| Physical state / projection | `Ns16550AStateInner`, `Console` and `CharDev` impls in `anemone-kernel/src/driver/serial/ns16550a.rs` | NS16550A uniquely owns MMIO/FIFO/line/TX；console only consumes output；TTY consumes `TtyPort` | raw personality remains; `read()` is `unimplemented!()`; claim/lease would add false mode truth | same state implements all traits；two narrow wrappers | 从同一 physical state 固定构造 output-only console wrapper 与 `TtyPort` wrapper；无 claim/lease/mode state |
| raw major 234 | `anemone-kernel/src/driver/serial/ns16550a.rs` imports/`CharDev` impl/minor allocation/bookkeeper/registration；`RAW_SERIAL` in `anemone-kernel/src/device/devnum.rs` | current char registry consumes raw endpoint | raw endpoint与TTY并存或 probe-order minor violates `TTY-PORT-001` | 保留兼容 endpoint；删除 raw projection | 删除 raw `CharDev` surface、dynamic minor与仅剩该 consumer 的 major 234 constant；generic CharDev不变 |
| boot line truth | `UartLineConfig`, stdout parser and apply path in `anemone-kernel/src/driver/serial/ns16550a.rs` | UART owns applied line；TTY reads immutable snapshot | probe-local config apply 后丢失；从 register 反推会复制 truth | 保存 applied snapshot；运行时反推 | 保存 probe 已验证并提交的 immutable snapshot；首版不增加 runtime apply/rollback |
| RX drain/error | `try_drain_irq()` and IRQ handler in `anemone-kernel/src/driver/serial/ns16550a.rs` | UART raw-handoff owner；future TTY deferred consumer | 当前 bytes被丢弃、无 batch budget/counter、每 IRQ debug output可能递归 TX | bounded drain to ring；IRQ policy processing | 固定 drain budget、drop-new overflow/counter、line-error counter，只有 empty-to-nonempty 窄通知；删除 IRQ printk |
| raw storage | `anemone-kernel/src/utils/ring_buffer.rs`; Box-at-create precedent in `anemone-kernel/src/fs/pipe.rs` | port owns bounded raw queue | IRQ `Vec`/`VecDeque` growth进入 allocator/OOM；overwrite破坏顺序 | preallocated ring；heapless queue；fallible growth | probe 时分配 `Box<RingBuffer<u8,N>>`，IRQ `try_push` 不分配，满时 drop-new；容量后续进入 kconfig |
| deferred carrier | `KThreadHandle::wake()` → `KThreadControl::wake()` → `Event::publish()` → `finish_wake_attempt()` → `wake_enqueue()` → Fair `enqueue_ready()` | kthread/Event/wait/scheduler各自拥有状态；TTY worker只消费ring predicate | `anemone-kernel/src/sched/class/fair/stride.rs`明确记录local IRQ-disabled `BinaryHeap::push`可能分配 | current per-port kthread；周期polling；新deferred lane | 按用户owner处置选用现有kthread wake；TTY不发明替代设施，也不声称修复scheduler的register issue。worker使用ring predicate，notification不携带数据 |
| threaded timer lane | `anemone-kernel/src/time/timer/threaded.rs` | timer core owns timer callbacks，不是 generic carrier | IRQ-off `VecDeque::push_back` 可增长，随后仍 `handle.wake()`；注释明确 not a workqueue | 借用 timer lane；新增 workqueue | 不借用；既 owner 错误又未消除 allocation 风险 |
| TX serialization | console/raw polling output and register write path in `anemone-kernel/src/driver/serial/ns16550a.rs`; console fan-out in `anemone-kernel/src/device/console.rs` | physical driver must own final serialization | 当前无 port TX lock；console/TTY逐字节交错；任意长 write 可形成无界 polling section | driver lock + bounded batch；TX queue/THRE IRQ；分离 locks | 先选 driver-owned IRQ-safe serialization + bounded batches；真实 drain/writable需求出现后再评估 TX queue |
| probe rollback | NS16550A probe publication/request_irq order and rollback TODO | driver owns pre-publish transaction | `request_irq`失败可留下 state/raw char/console；IRQ enable前无TTY consumer | fallible work first；现有顺序 | 先构造 state/ring/worker/binding，完成 fallible work后 request IRQ，再做不可失败 commit；exact shape留待Stage 1 resolution |

**Transport conclusion:** preallocated storage有可执行路线；deferred carrier按用户owner处置选用现有
`KThreadHandle::wake()`。它底层当前仍有register记录的scheduler IRQ-off allocation风险，但这属于
wait-core/scheduler owner，不驱动TTY新增carrier或停止Stage 0。TTY自身IRQ/storage路径必须保持有界、
预分配、predicate-driven，timer lane与周期polling均不采用。

### 3. Endpoint / boot matrix

| 项目 | Live symbol / path | Owner / consumer | Failure signal | 候选路线 | 推荐路线 / 未决项 |
| --- | --- | --- | --- | --- | --- |
| Firmware identity | `anemone-kernel/src/device/discovery/open_firmware/chosen.rs`; OF alias/node handle in `anemone-kernel/crates/device-tree`; `FwNode::equals` and platform full name | firmware/platform owns physical identity；driver consumes | `GeneralMinorAllocator` 按 probe 完成顺序编号 | fixed platform table；firmware alias；sorted allocator | driver-local canonical OF identity/table生成稳定 `N`；chosen handle只用于同次启动重验 |
| chosen stdout | console discovery before probe in `main.rs`; `stdout_config()` in NS probe | firmware owns chosen identity；console owns selected console；TTY revalidates endpoint | `ConsoleDesc`无terminal identity；no-chosen fallback依赖注册顺序 | registration携带窄 identity；TTY复制selection | console暴露 immutable selected-terminal identity；TTY按自身registry重验；fallback按确定性port identity |
| devnum | `TTY=4`, `RAW_SERIAL=234` in `anemone-kernel/src/device/devnum.rs`; no 5:1 constant | TTY owns ttyS numbering；console owns console node | raw dynamic minor漂移；TTY接管`/dev/console` | 复用 raw；Linux baseline | `/dev/ttyS<N>` 为 `4:(64+N)`，`/dev/console` 为 `5:1`，由各 owner构造；删除 raw 234 |
| devfs publish | `DevfsPublish` / `DevfsNodeOps` and `devfs::publish` in `anemone-kernel/src/fs/devfs/` | devfs owns visible registry；TTY/console own open behavior | publish后无unpublish；半初始化 ops 会永久可见 | generic CharDev；owner-specific ops | 两个 owner各自提供专属 ops；publish前闭合 terminal/ring/consumer/open，`devfs::publish` 是线性化点 |
| `/dev/console` | `anemone-kernel/src/device/console.rs` has anonymous files but no `DevfsPublish` | console owner | 当前无永久node；stdin EOF、poll未实现、ioctl ENOTTY | console node；TTY delegation | 首版由console owner发布output surface，不伪装为Terminal；delegation留后续真实consumer |
| boot fd 0/1/2 | `exec_init_proc()` in `anemone-kernel/src/main.rs` | boot coordinator consumes selected identity；TTY supplies real Terminal files | anonymous stdin永久EOF；stdout UTF-8-only；不具TTY语义 | keep anonymous；open console；direct Terminal files | console输出immutable identity，TTY revalidate后按 Read/Write/Write 安装同一Terminal的正常files |

**Endpoint conclusion:** live OF identity和devfs单向publish足以形成稳定 endpoint 路线，不需要并发
probe顺序、runtime unpublish或TTY接管console owner。该矩阵未命中 identity/projection stop condition。

### 4. Relation / topology matrix

| 项目 | Live symbol / path | Owner / consumer | Failure signal | 候选路线 | 推荐路线 / 未决项 |
| --- | --- | --- | --- | --- | --- |
| Stable identity | global topology registries and `ThreadGroup` `TidHandle` under `anemone-kernel/src/task/topology/` | topology owns Session/ProcessGroup/ThreadGroup identity and membership | TTY保存裸SID/PGID或复制membership | numeric cache；stable Arc/handle | relation保存stable identity；numeric ID只做ABI lookup并由owner重验 |
| Reuse-safe lookup | `with_child_status_transaction()` and owner-local registry checks | topology owner | numeric ID reuse使旧foreground指向新group | raw equality；lookup + `Arc::ptr_eq` | 复用现有lookup + identity equality模式；TTY不取得topology guards |
| mutation | `create_session()` and `move_to_process_group_if()` | topology owns setsid/group move transaction | Session与Terminal各保存mutable binding/foreground | shared mutable fields；narrow owner API | Stage 3解析relation owner与窄query/decision/revalidation API，不存完整Task |
| Signal decision | ThreadGroup-shared disposition, per-task mask, process-group targeting and `RestartSyscall::Idempotent` | Signal/topology/jobctl各自拥有decision/effect | TTY缓存mask/disposition或直接改stop truth | TTY policy；guards-out decision | short-lived caller snapshot → Signal/topology owner decision → relation generation revalidation → commit/retry |
| lifecycle | `kernel_exit_group()` and last-member `kernel_exit()` / topology detach | ThreadGroup lifecycle owns Exiting/Exited；TTY owns relation discoverability | cleanup无唯一owner或先发effect再撤销relation | exit hook直接改TTY；narrow notification | Stage 3在owner-safe hook先撤销relation/foreground可发现性，再guards-out effects；首版不发detach SIGHUP/SIGCONT |

**Relation conclusion:** live topology/Signal接口可以通过窄 owner API 闭合 target，不要求 relation 双重
truth或 `Terminal` 保存完整 `Task`。最终 API 与 cleanup hook 留待 Stage 3 resolution，不提前冻结。

### 5. Oracle / harness matrix

| Case family | Provenance / live route | Expected result | Current gap / failure signal | Stage input |
| --- | --- | --- | --- | --- |
| Linux UAPI | Linux 6.6.32 `include/uapi/asm-generic/{termbits.h,ioctls.h}` | 冻结target内termios/ioctl值、结构和errno | 不从host libc header猜ABI | Stage 2定向UAPI/tty-test oracle |
| BusyBox source/config | final testsuite commit `d69becb811573aa789a788e2940fa5ed8f9388f3`; RV64/LA64 configs enable `CONFIG_ASH_JOB_CONTROL`, `CONFIG_VI`, `CONFIG_STTY` | ash使用`tcgetpgrp/tcsetpgrp`并回收foreground；vi使用raw termios/winsize/SIGWINCH；stty执行termios read-modify-write | 没有用户提供的immutable executable artifact | artifact identity/applet/runtime核对是后续 Gate P3/cutover 外部前置，不阻塞 Stage 0 本身 |
| `tty-test` | RFC-defined future app；current repo不存在 | 自动判定data plane、termios/readiness、relation、三分支`TIOCSPGRP`、signals/cleanup | 不能用success stub或shell prompt替代 | 相应 stage resolution冻结最小case与逐项PASS/FAIL |
| rootfs/launcher | `just rootfs mkfs -c <manifest>`；rootfs manifest owner in `conf/rootfs/` and xtask rootfs task；current pretest init → user-test → chroot chain | 独立轻量manifest按`[build].name`隔离输出，launcher建立session/controlling/foreground后exec ash | current pretest固定到user-test，不适合交互；不得修改默认pretest flow | 新增独立repository-owned manifest/app route；wrapper必须显式选择，不引用个人`etc/`路径 |
| runtime acceptance | future repository wrapper + explicit user checklist | automated QEMU smoke；vi raw/canonical/winsize；ash反复`Ctrl-Z/jobs/fg/bg/fg`并回收foreground | artifact缺失且本次docs-only stage不得运行 | Stage 2/4分别记录agent-run、user-run与Not Run；当前全部Not Run |

**Oracle conclusion:** 核心 BusyBox 包络有可构造 source oracle；缺失 executable artifact 是后续
Gate P3/cutover 前置条件，不是 Stage 0 blocker。repository rootfs owner允许独立轻量manifest而不改变
pretest默认流程。

### 6. Module-boundary conclusions

| Surface | Current responsibility mix | Conclusion | Stage input / failure signal |
| --- | --- | --- | --- |
| `driver/serial/ns16550a.rs` | line parsing、registers、console/raw-char projection、probe、identity/bookkeeping、IRQ与KUnit | Stage 1首个checkpoint应先做same-owner behavior-preserving split-only；候选角色为regs/port/probe，名称不冻结 | 必须保留LoongArch early console对`Ns16550ARegisters`的窄使用；不得顺便建generic serial framework或改public trait |
| `device/console.rs` | registry/selection、fan-out、early→normal切换、anonymous stdio | Stage 1不因TTY先拆；Stage 2加入selected identity、`/dev/console`和boot handoff前再独立判断split-only | `register_console` surface或selected identity contract变化不是纯拆分，须进入Ready manifest/review |
| VFS ctx | generic status/ioctl/open边界已经足以从TTY entry派生caller | 不拆、不扩CharDev；新增TTY专属FileOps/open provider | 若必须扩大所有FileOps caller surface，触发Stage stop |
| task topology/Signal | owner边界清楚，但尚无TTY-specific decision API | Stage 3 resolution只增加窄query/decision/revalidation surface；不把结构性API变化伪装split-only | public API/owner/shared contract变化须单独manifest/review |

## Carrier ownership finding

**Initial Keter classification — no allocation-free IRQ-to-process-context carrier.** 预分配 `RingBuffer` 能闭合 raw storage，
但不能闭合 notification carrier。当前 per-port kthread route 必经：

1. `KThreadHandle::wake()` / `KThreadControl::wake()`；
2. `Event::publish()` 与 wait-core wake commit；
3. `wake_enqueue()` 的 local IRQ-disabled scheduler placement；
4. Fair `BinaryHeap::push()`，live source明确记录 growth 可能分配。

唯一相似的 threaded timer lane不是generic workqueue，而且在 IRQ-off context中执行可增长的
`VecDeque::push_back()`，随后仍走同一 kthread wake。全树没有其它 generic bottom-half/workqueue。
让worker周期poll ring会违反R0的no-polling progress boundary，让IRQ tail执行line discipline则违反
hard-IRQ correctness boundary。

初审据此把现有repository carrier底层实现债务归入Stage 0 stop condition，并形成下列备选范围。
该分类随后由用户按owner边界处置并更正，见后文；证据本身保留，不被改写成scheduler已经安全。

## Route options and expansion report

### Route A — allocation-free wake-placement prerequisite（初审推荐，未采纳为TTY前置gate）

由 scheduler/kthread owner 在独立前置 gate 中证明：预创建worker从 hard IRQ 被唤醒到 runnable placement
的全链 allocation-free，不进入 OOM、普通日志、复杂 drop 或 sleepable lock。TTY worker随后仍使用
ring-nonempty predicate，notification不携带数据。

**Proposed scope:** `sched/{wait,processor,class}`、`task/kthread`与定向KUnit/source proof；精确文件须由
该owner gate重新审计并冻结。若形成跨consumer共享保证，应由scheduler current-contract流程拥有，
不能写成TTY contract已经生效。

### Route B — allocation-free IRQ deferred lane（不推荐）

新增generic或TTY-private IRQ deferred lane会扩大IRQ/scheduler shared infrastructure、lifecycle和
public/shared surface；当前没有第二个真实consumer证明generic abstraction必要。只有Route A被证伪且
独立RFC/owner review接受后才能考虑，不能作为TTY局部兼容桥。

### Route C — polling或IRQ-tail policy（拒绝）

周期polling违反durable notification target；在IRQ tail执行discipline、Event、Signal或topology工作
违反hard-IRQ边界。它们不能用于关闭Stage 0或进入Stage 1。

**Contract impact:** 初审没有发现R0 target、owner、ABI、cutover unit或accepted limitation变化；
全部`TTY-*`继续Not Cut Over。用户后续决定不把跨scheduler/kthread owner的修复作为TTY Stage 0
前置gate，因此本节只保留被拒绝的扩展分析。

**Proposed validation:** 这里列出的scheduler全链proof/stress归wait-core/scheduler owner，不进入TTY
Stage 1 manifest。后续Stage 1只验证TTY自身fixed-ring FIFO/drop-new、empty→nonempty、通知前后
publication、concurrent drain和bounded IRQ budget，以及repository wrapper/QEMU RX burst。

## User disposition and correction - 2026-07-23

用户明确决定TTY使用现有`KThreadHandle::wake()`；该API底层Fair runqueue的IRQ-off allocation问题
不是本RFC的owner责任，不应让TTY停摆，也不应驱动TTY临时发明workqueue、softirq或专用scheduler
设施。`ANE-20260622-IRQ-OFF-HEAP-ALLOCATION`继续由wait-core/scheduler owner在后续独立工作中修复。

据此更正初审分类：现有kthread wake是repository-owned窄notification boundary，可作为Stage 1的
单一路线输入；TTY不穿透其内部实现，也不声称修复或关闭register issue。TTY必须预分配raw ring，
只在empty-to-nonempty边界调用wake，以ring nonempty predicate作为durable work truth，并保证自身
IRQ drain、storage、counter与notification调用前后不新增allocation、复杂drop、普通日志或sleepable
lock。该处置不改变R0 target、owner、ABI、cutover或acceptance boundary，`KETER-008`保持Neutralized。

## Review, validation and stop-condition audit

**Review:** R0 acceptance review没有target-level blocker。Stage 0 execution review的carrier finding
已按用户owner disposition neutralize；未发现Apollyon、开放Keter或Euclid。VFS、endpoint、relation、
oracle与模块矩阵没有其它Euclid以上finding。`KETER-008`已同步implementation evidence与重新打开边界。

**Stop audit:** caller handoff、port identity、fixed console + `TtyPort` projection、relation ownership和
BusyBox oracle均有target-preserving路线。deferred carrier按用户处置选用现有kthread wake；TTY没有借用
threaded timer lane、周期polling、success stub、test-specific fallback或自建generic infrastructure。
不存在其它未处置stop condition。

**Validation:** `git diff --check`通过；两个新文件分别以
`git diff --no-index --check /dev/null <file>`检查，退出1仅表示新文件有diff且没有whitespace诊断。
`mdbook build docs`通过，只报告既有search index过大警告。RFC/transaction双向链接、SUMMARY、
transaction index、双周devlog、Stage 0/1 heading、`KETER-008`、
`ANE-20260622-IRQ-OFF-HEAP-ALLOCATION`、Preserve contract目标和矩阵source paths完成定向存在性审计；
未发现missing target。最终status/write-set审计确认只有本文列出的docs发生变化，kernel、apps、rootfs、
test profile、build config、register和current contracts均无diff。

没有运行kernel build、KUnit、QEMU、BusyBox、LTP、LA64 runtime或硬件测试；source audit与mdBook
build不冒充runtime proof。

## Result and handoff

Stage 0 **Closed**。六类矩阵、owner/route/module结论、review与stop audit均已完成；R0保持
Accepted for Implementation，两个cutover和全部`TTY-*`保持Not Cut Over。Stage 1仍为Outline /
Not Started，未执行Stage 0 -> Stage 1 Resolution Gate。

下一步只能在新的明确授权下执行独立的Stage 0 -> Stage 1 Resolution Gate。该gate应把预分配raw ring、
现有`KThreadHandle::wake()` consumer、driver-owned bounded TX serialization、NS16550A same-owner
split-only checkpoint、pre-publish rollback与本次oracle输入解析为完整Stage 1 Ready和exact manifest；
不得把Stage 0 closure或用户carrier处置当作Stage 1授权。

## Stage 0 -> Stage 1 Implementation Resolution Gate

**Authorization and entry:** 用户在Stage 0独立关闭后明确授权解析Stage 1并完成resolution gate；没有
授权Stage 1代码实现。入口为`dev/drc/omega@9fd95821`，worktree clean；Stage 0 commit只修改RFC/devlog
文档，没有kernel/apps/rootfs/test/build/register/current-contract diff。

**Preflight evidence:** 重新读取R0 target/invariants、Stage 0六类矩阵、`KETER-008`处置、
`ANE-20260622-IRQ-OFF-HEAP-ALLOCATION`、current contracts和live source。当前仍不存在`device::tty`；
NS16550A单文件继续混合register/probe/raw CharDev/console/IRQ，raw major 234只由该driver与devnum KUnit
引用；`RingBuffer<T,N>`固定容量但不自带并发；`KThreadHandle::wake()`/`KThreadCtx::wait_until()`提供既定
notification-plus-predicate边界；`request_irq()`没有对应free/unregister surface并在成功时直接unmask。
build owner仍由`just`/xtask、tracked`conf/.defconfig`和ignored generated defs组成，active config启用KUnit。

**Resolved decisions:**

| 项目 | Stage 1单一路线 | 边界 / 失败信号 |
| --- | --- | --- |
| Module pressure | 先将`ns16550a.rs`同owner拆为`mod/regs/port`，保持`crate::driver::Ns16550ARegisters`路径 | early console或public trait必须变化则split checkpoint停止 |
| `TtyPort` | crate-private identity/predicate/dequeue/TX-progress capability；`device::tty`拥有unpublished attachment与consumer worker | 不暴露register/lock/container/Task/FileOps/termios/Signal |
| RX storage | probe-time `Box::try_new`固定ring，port IRQ-safe lock，FIFO/drop-new，capacity 4096；IRQ budget 256 | overwrite、silent drop、IRQ growth/OOM side effect停止 |
| Carrier | 每port现有kthread；只在empty-to-nonempty后、raw guard外调用`KThreadHandle::wake()`；ring predicate是durable truth | 不修改wait-core/scheduler，不使用poll/timer/workqueue替代 |
| TX | physical port唯一IRQ-safe lock；16-byte batch、每byte 65536次poll上限、partial progress/counter | 任意长度IRQ-off section、递归printk或console/TTY旁路停止 |
| Identity / line | OF node canonical full path拷入固定容量identity；driver保存immutable applied-line snapshot | 超长/非OF在attach前失败；不使用probe-order minor或局部basename |
| Raw projection | 删除NS16550A CharDev/registration/minor bookkeeper与仅剩consumer的major 234 | generic CharDev保持不变，任何raw endpoint残留阻止Stage关闭 |
| Rollback | 全部fallible allocation/attach/spawn在request IRQ前；失败撤unpublished registry并stop/join；request成功后只做infallible commit | 无free_irq，因此request后新增fallible步骤立即停止 |

**Checkpoint and manifest result:** canonical plan的
[Stage 1 Ready](../../rfcs/tty-subsystem/implementation.md#6-stage-1-readyunpublished-transport-vertical-slice)
已经冻结三个checkpoint：C1 same-owner split-only；C2 dormant TTY port/attachment core与fake-port KUnit；
C3 NS16550A production wiring、四项Kconfig、raw 234删除、IRQ/TX/rollback及RV64 burst/LA64 build。完整逐文件
manifest、validation-only inputs、review责任、bridge删除点、精确验证、停止和退出条件只由该节拥有，
本文不复制第二份计划。

**Contract/review result:** 未发现Apollyon、开放Keter或Euclid；resolution不改变R0 target、owner、ABI、
visible semantics、contract delta、两个cutover unit或acceptance boundary，因此不增加RFC修订，不更新
tracking issue或current contracts。`KETER-008`保持Neutralized；scheduler/wait-core的IRQ-off allocation
问题继续Open且不进入Stage 1 manifest。Stage 1 contract cutover为None，全部`TTY-*`仍Not Cut Over。

**Validation boundary:** 本gate修改canonical implementation plan与本transaction，并只在RFC入口、RFC
总索引和当前双周devlog同步Stage 1 Ready / Not Started导航；这些状态写回不改变target或source manifest。完成
`git diff --check`、`mdbook build docs`和定向heading/link/status/manifest/source-path审计后才可关闭；不运行
kernel build、KUnit、QEMU、BusyBox、LTP、LA64 runtime或hardware test，这些是Ready Stage 1的未来floor。

**Resolution validation:** `git diff --check`通过；`mdbook build docs`通过，只报告既有large search-index
warning。生成HTML中的Stage 1 heading与RFC/transaction两条anchor link一致；当前RFC入口、RFC总索引、
双周devlog、implementation和transaction均显示Stage 1 Ready / Not Started。四项Kconfig owner、现有
ring/kthread/IRQ/LoongArch/bootstrap/wrapper validation-only路径与五个docs write-set路径完成定向存在性审计；
最终`git diff --name-only`确认没有kernel、apps、rootfs、test profile、build config、register或current
contract diff。没有运行任何Ready Stage 1代码验证，docs build与source audit不冒充build/runtime proof。

**Result:** Stage 0 -> Stage 1 Resolution Gate完成，Stage 1为**Ready / Not Started**。用户本轮授权到此
结束；Checkpoint 1、整个Stage 1或连续checkpoint均未激活。下一步只能在新的明确授权下进入Stage 1
实现，并在transaction记录实际activation point。

## Stage 1 / Checkpoint 1 activation - 2026-07-23

**Authorization and entry:** 用户明确授权建立transaction设施并完成Stage 1第一个checkpoint。入口为
`dev/drc/omega@0b5186a8`，worktree clean；active `kconfig`为`qemu-virt-rv64-pretest`、release、KUnit
enabled。Checkpoint 1按canonical plan第6.3节单独激活，不授权Checkpoint 2或连续checkpoint执行。

**Preflight:** live source仍为657行`driver/serial/ns16550a.rs`；
`crate::driver::Ns16550ARegisters`只由LoongArch bootstrap经既有re-export使用。实现只允许删除该旧文件、
新建`ns16550a/{mod.rs,regs.rs,port.rs}`并追加本文：`mod.rs`保留option parsing、probe/commit和KUnit，
`regs.rs`拥有register access与line programming，`port.rs`拥有现有physical state、console/raw CharDev、
IRQ和minor bookkeeper。除子模块路径、`use`与必需的`pub(super)`可见性外，不改变函数体、assertion、日志、
probe/registration/request-IRQ顺序、public re-export、owner或行为。

**Contract Cutover:** None。全部`TTY-*`保持Not Cut Over。

**Next:** 在上述split-only子集内完成机械拆分、source/diff review与Checkpoint 1 validation floor；命中
public trait、register API、owner或行为变化时立即停止。Checkpoint 1关闭后不自动激活Checkpoint 2。

## Stage 1 / Checkpoint 1 split-only closure - 2026-07-23

**Change:** `driver/serial/ns16550a.rs`机械拆为`ns16550a/{mod.rs,regs.rs,port.rs}`。`mod.rs`保留
option parser、probe/commit、driver registration和原有KUnit；`regs.rs`保留register access、line
programming与公开`Ns16550ARegisters`；`port.rs`保留physical state、raw CharDev、console、IRQ和minor
bookkeeper。没有引入TTY、改动raw endpoint、锁、日志、registration/request-IRQ顺序或既有rollback TODO。

**Review:** diff/source review未发现Apollyon、Keter或Euclid。option/parser、register实现、
`DriverOps`/`PlatformDriver`、两项KUnit、Console/CharDev、IRQ handler与init均逐段和入口`HEAD`一致；
KUnit数量与assertion不变。唯一非路径差异是子模块间必需的`pub(super)`，其可见范围仍限制在同一
`ns16550a` owner；`crate::driver::Ns16550ARegisters`继续经原路径re-export，public API未扩大。

**Validation:** `git diff --check`与三个新文件的no-index whitespace check通过；
`just fmt kernel --check`通过。入口`qemu-virt-rv64-pretest`下`just build`以release、KUnit、ext4和
irqsave features编译/link通过；首次沙箱内build在lwext4 C编译阶段被seccomp以`Bad system call`终止，
获批沙箱外同命令通过。切换LA64后，`just build`在kernel compile前因缺少
`build/rootfs/pretest-la64/rootfs.img`停止；非sudo rootfs materialization完成app staging后由supermin
权限失败，sudo重试停在密码提示时，用户明确裁定本次只要求RV64并取消LA64构建，因此未提供密码并终止。
随后恢复入口RV64配置，最终`just build`再次通过。LA64 compile/link、LA64 runtime、QEMU boot、KUnit
runtime、BusyBox与LTP均未运行；RV64 build不冒充这些层级的证明。

**Feedback:** Execution Fact。用户对本checkpoint作出一次性validation disposition：本次split未修改
LoongArch source，接受不运行LA64 build；canonical Stage 1后续checkpoint floor未被本文改写。target、
owner、ABI、visible semantics、acceptance boundary和resolved source manifest均未变化。

**Contract Cutover:** None。全部`TTY-*`保持Not Cut Over，current contracts与register未修改。

**Result / Next:** Checkpoint 1 **Closed**，Stage 1保持Active。Checkpoint 2仍为Not Started，未建立
`device::tty`、unpublished attachment、worker或任何Stage 1 bridge；后续必须单独激活Checkpoint 2。

## Stage 1 / Checkpoint 2 activation - 2026-07-23

**Authorization and entry:** 用户明确授权完成Stage 1剩余两个checkpoint；依照一次只激活一个checkpoint的
合同，本节只激活Checkpoint 2。入口为`dev/drc/omega@0b3d5da6`，worktree clean；active `kconfig`为
`qemu-virt-rv64-pretest`、release、KUnit、ext4和irqsave enabled。

**Preflight:** canonical Stage 1第6.3节仍将本checkpoint冻结为dormant TTY port/attachment core；允许写集
只有`device/mod.rs`、新建`device/tty/{mod.rs,port.rs}`和本文。现有`KThreadHandle::wake()`是纯notification，
`KThreadCtx::wait_until()`提供register-plus-recheck，`request_stop()`加`wait_exited()`是同步join边界；因此
registry只需拥有unpublished identity索引，attachment拥有撤索引、stop和join，worker只依赖port RX
predicate/dequeue，notifier不携带business truth。production NS16550A继续走Checkpoint 1 raw路径。

**Contract Cutover:** None。全部`TTY-*`保持Not Cut Over；current contracts与register不修改。

**Next:** 在上述C2写集内实现crate-private capability、unpublished registry、Stage 1-only drain sink和fake-port
KUnit。若需要观察Task/scheduler内部、扩大kthread/Event/shared API、引入polling watchdog或修改production
driver，立即停止；Checkpoint 2关闭后不自动激活Checkpoint 3。

## Stage 1 / Checkpoint 2 dormant attachment closure - 2026-07-23

**Change:** 新增crate-private `device::tty::{TtyPortId,TtyPort}`窄capability、按immutable identity索引的
unpublished registry、`TtyPortAttachment`、`TtyRxNotifier`与每attachment kthread。worker只以
`rx_pending()`为durable truth，向固定64-byte栈上batch按序dequeue；Stage 1 sink仅用Relaxed diagnostic
counter记录并丢弃，Stage 2 consumer replacement前不得发布。attachment abort先撤registry visibility，
释放guard后再request-stop/join；notifier只封装`KThreadHandle::wake()`。

**KUnit:** owner-local fake port使用固定容量`RingBuffer`，覆盖duplicate identity直到abort、notification在
worker wait前后、drain期间追加RX、predicate持续为true时跨batch FIFO drain，以及abort撤registry并确认worker
exit。fake guard内不分配，不新增通用test framework、callback、polling watchdog或Task/scheduler观察面。

**Review:** source/lock/lifecycle review未发现Apollyon、Keter或Euclid。registry guard只覆盖duplicate
validation/insert/remove，不跨worker spawn、wait、port drain、stop或join；worker不持registry guard，port
capability不暴露register/lock/container；diagnostic counters明确不参与predicate、ordering或state transition。
`Drop`完成cleanup后才assert removal/exit invariant。production NS16550A、raw endpoint、boot行为、kthread/Event
owner与shared API均无diff，未命中C2 bridge/stop条件。

**Validation:** 两个新文件的no-index whitespace check无诊断，`git diff --check`通过；串行
`just fmt kernel`后`just fmt kernel --check`通过。沙箱内首次`just build`在lwext4 C编译被seccomp以
`Bad system call`阻断，获批沙箱外同命令以RV64 release、KUnit、ext4和irqsave编译/link通过且无告警。
`./scripts/run-user-test-rv64.sh etc/preliminary/images/sdcard-rv.img build/tty-stage1-c2-rv64.log`
重建rootfs、本地测试盘副本和kernel后启动QEMU；6项新增TTY KUnit全部`ok`，全量218项打印
`All tests passed!`，未见panic。达到KUnit证据点后经QEMU monitor `quit`正常退出；其后已开始的userspace/LTP
输出不计入本checkpoint证据。没有运行LA64 build/runtime、BusyBox、硬件或C3 production RX probe。

**Contract Cutover:** None。全部`TTY-*`保持Not Cut Over，current contracts、register和RFC target未修改。

**Result / Next:** Checkpoint 2 **Closed**，Stage 1保持Active。Checkpoint 3仍为Not Started；production
NS16550A仍走Checkpoint 1保留的raw CharDev路径。后续必须单独执行C3 preflight/activation，不得把C2 closure
视为production transport授权。

## Stage 1 / Checkpoint 3 preflight stop - 2026-07-23

**Entry:** 入口为`dev/drc/omega@ec0c9945`，worktree clean。依照一次只激活一个checkpoint的合同，本节先
执行只读C3 preflight；没有激活production source修改。

**Blocking evidence:** live boot顺序在`bsp_kinit()`中先执行`of_platform_discovery()`和
`probe_virtual_devices()`，随后才完成timer/percpu/local-IRQ初始化并调用`init_kthreadd()`。platform bus在
`register_device()`中同步调用匹配driver的`probe()`，因此NS16550A `probe()`确定发生在kthreadd初始化前。
C3 canonical route要求NS16550A在`request_irq()`前调用`attach_unpublished_port()`，而C2 attachment同步使用
`KThreadBuilder::spawn()`；live `kthreadd::submit()`对未初始化`KTHREADD`执行correctness assertion
`kthread spawn before kthreadd initialization`。按现状接线会在boot中panic，不能形成C3 candidate。

**Stop audit:** 不能把worker简单延后到late initcall：若early probe先request IRQ，late worker spawn仍是
irreversible IRQ commit后的fallible步骤，直接命中C3 stop；若early probe先安装driver state/register console而
late阶段再spawn/request IRQ，worker或IRQ失败时没有console/driver unpublish/unbind rollback，probe却已经向bus
返回成功，同样破坏pre-publish transaction。周期poll、IRQ-tail sink、optional notifier或raw CharDev并行消费都被
R0/Stage 1拒绝。解决该冲突必须修改`main.rs`的boot ordering、platform bus/driver probe coordination、kthread
early-publication contract或等价shared owner；这些文件与owner均在C3 resolved manifest之外，并命中
“必须修改kthread/shared contract才能继续”的checkpoint停止条件。

**Expansion report:** 尚未批准或实施任何扩展。候选方向只有：(A)由boot/kthread owner重新排序physical discovery、
CPU/local-IRQ readiness与kthreadd publication，并证明不会让其它platform/virtual driver或console初始化退化；
(B)由device/driver owner增加可回滚的deferred-probe/finalize协议，使NS16550A直到kthreadd ready后才向bus提交probe
成功；(C)重新解析C3 pre-publish route，但必须仍保证worker/consumer在RX enable前可达、request IRQ后没有
fallible half-commit、request失败无registry/worker/console/driver残留。A/B跨owner和shared surface；C若无法保持
这些边界则进入Target Renegotiation Gate，不能由implementation自行接受。

**Contract and validation impact:** 当前无contract cutover，全部`TTY-*`仍Not Cut Over；production raw major
234、console和IRQ行为保持`ec0c9945`入口状态。若批准A/B扩展，至少需要boot-order/source-lock审计、kthread
creation/stop KUnit、全部C3 focused KUnit、RV64 release build和规定的QEMU boot/KUnit/RX burst，并证明所有受影响
driver仍在合法ready phase probe；依用户本轮处置不运行LA64 build，不能把RV64证据外推为LA64 proof。若批准C，
需先在canonical `implementation.md`更新route、manifest、rollback/stop和验证，再重新激活C3。

**Result:** Checkpoint 3保持**Not Started / Stopped at Preflight**，没有C3 source diff、bridge、Kconfig或
contract变化。Stage 1保持Active，等待owner/write-set扩展或C3 route correction的明确批准；不得自动进入Stage 2。

## Stage 1 / Checkpoint 3 route correction and activation - 2026-07-23

**Authorization and entry:** 用户明确拒绝引入generic deferred-probe framework，并判断仅为UART建立新的
early-kthread boot phase不自然；随后批准保持live boot/device顺序、采用driver-local quiescent probe与Late
activation bridge并继续Checkpoint 3。入口为`dev/drc/omega@2d8a3671`，worktree clean；此前stop commit只
记录证据，没有C3 source、config、contract或bridge diff。

**Corrected route:** early synchronous NS16550A probe完成firmware/identity/line解析、MMIO mapping、固定
port/ring和IRQ-private对象分配，关闭UART RX interrupt，安装driver-local `Quiescent` port并注册output-only
console；它不request IRQ、不启RX、不进入TTY registry。`Late` initcall在`kthreadd`与全部CPU local init完成后
遍历本driver的quiescent ports，依次完成unpublished attachment、worker/notifier binding和`request_irq()`；
request成功后只做不可失败的attachment install、RX enable与`Active` phase提交。attach/request失败先abort
attachment并stop/join worker，不留下registry、worker、IRQ或RX enable；允许保留的只有有界boot-lifetime
MMIO、port、driver binding和TX-only console，且这类失败不能满足Stage 1 closure。

**Bridge disposition:** `Quiescent -> Active`只解决当前bus无法表达`kthreadd` dependency的问题，不是第二套TTY
truth、generic lifecycle或长期deferred-probe替代。未来platform bus具备dependency-aware deferred probe/retry
后，NS16550A probe应在dependency未满足时defer，ready重试时复用同一activation transaction，并删除Late
initcall与phase bridge。IRQ永远不读取optional notifier：notifier绑定前没有已注册IRQ，request成功后notifier
与attachment保持到重启。

**Contract / owner review:** 修正保持R0 owner、ABI、visible semantics、console + `TtyPort`固定projection、
cutover unit与acceptance boundary；platform driver binding不是TTY endpoint publication，quiescent port不向
TTY/devfs暴露且不接收RX。精确pre-publish rollback shape属于R0保留的implementation latitude，因此本次只更新
`implementation.md`的Stage 1 route/validation/manifest，不递增RFC revision，不修改invariants、tracking issues、
current contracts或register。代码write set仍限制在既有C3 manifest；不修改main、bus、console、IRQ core、
kthread/wait/scheduler或任何shared/public API。

**Docs write-set correction:** RFC入口、RFC总索引、当前双周devlog与transaction index仍错误显示Stage 1
Not Started。获准route correction把这些same-owner导航文件加入docs-only write set，只同步Stage 1 Active、C2
Closed与C3 Active；不改target或source contract。用户本轮明确不要求LA64 build/runtime，canonical C3/Stage 1
validation floor据此记录为RV64-only，最终报告不得把RV64证据外推为LA64 proof。

**Activation:** Checkpoint 3现为**Active**。下一步只在修正后的C3 manifest内实现quiescent probe、Late
activation、fixed console + TTY projections、四项Kconfig、RX/TX/IRQ transaction与raw 234删除；若Late
activation需要其它Late consumer顺序、shared framework/API、request成功后的fallible步骤或任何RX-before-
consumer窗口，立即重新停止。Checkpoint 3关闭后只审计Stage 1退出条件，不自动进入Stage 2。

## Checkpoint 3 corrected-route review - 2026-07-23

**Review pause:** 在source修改前暂停C3 activation并对修正路线做独立owner/lock/lifecycle review。发现两个
Keter和一个证据边界缺口，均可在既有C3 owner/write set内关闭，不需要main、driver framework、console、IRQ
core、kthread或shared/public API扩展。

**Keter 1 / driver snapshot:** `Driver::for_each_device()`在callback期间持有`DriverBase.devices`的IRQ-save
read guard，不能直接在callback内allocation、attach、spawn、request IRQ或join。修正为static driver owner的
two-pass snapshot：第一遍锁内只计数，锁外fallible reserve，第二遍锁内只clone device `Arc`并push到已预留
capacity；释放guard后逐个activation。boot期无并发NS16550A device registration是本路线的source invariant，
push前用assert暴露容量漂移；不修改`DriverBase`。

**Keter 2 / activation owner:** `Ns16550ADevice`中的`SpinLock<Option<TtyPortAttachment>>`是唯一
Quiescent/Active truth；`None`与`Some`分别表示未激活和已提交，不增加独立phase字段。attachment留在
device state，不能放进physical port，以避免`port -> attachment -> endpoint -> port`强引用环。成功顺序冻结为
attach、构造IRQ context、request IRQ、slot提交`Some`、enable RX；IRQ不读取slot。失败保持slot `None`，先撤
registry再stop/join。

**Evidence correction:** bounded TX只证明port-owned TX lock按batch/poll参数有界；generic
`console::output()`持有外层console registry IRQ-save guard的既有行为不属于C3证明。当前target只要求physical
port serialization与TTY write分批，故无需扩展console owner；若未来要求任意console record端到端IRQ-off
latency，必须独立申报。

**Result:** 三项finding已写回canonical Stage 1 route与source proof，R0 target、owner、ABI、visible semantics、
acceptance和代码manifest不变。Checkpoint 3恢复**Active**，可以进入源码实现。

## Stage 1 / Checkpoint 3 production transport closure - 2026-07-23

**Change:** NS16550A early probe现在从OF canonical full path构造`TtyPortId`，应用并保存immutable line
snapshot，预分配4096-byte raw RX ring、physical port和fixed console/TTY projections，在IER/RX关闭时只安装
driver state与output-only console；它不再注册raw `CharDev`、分配dynamic minor或request IRQ。static driver
owner的Late initcall用count/reserve/snapshot两遍遍历取得锁外device list，再逐个执行unpublished attach、worker/
notifier binding、IRQ private context构造与`request_irq()`；成功后只提交
`SpinLock<Option<TtyPortAttachment>> = Some`并启RX，失败则abort attachment、撤registry并stop/join worker。

physical port以单一IRQ-safe fixed ring保存RX，IRQ每次最多读取256 bytes，FIFO满时drop-new并累计diagnostic
counter，只在empty-to-nonempty且释放raw guard后wake；IRQ不分配、不format、不打印，也不访问TTY policy。
console与TTY共用同一TX入口，每个port-owned lock section最多16 bytes、每byte最多65536次poll，timeout返回partial
progress并计数。Stage 1 worker继续以ring nonempty为durable predicate；production port首次成功drain后只输出一条
process-context摘要，fake-port KUnit不消费或复制该production诊断。`RAW_SERIAL` major 234、NS16550A
`CharDev`、minor allocator/bookkeeper和IRQ lookup旁路已经删除；generic char/device/console/IRQ框架不变。

四项non-zero kconfig参数由`conf/.defconfig`与xtask config owner生成；显式零值被拒绝，active kconfig缺省字段
从defconfig解析。generated `kconfig_defs.rs`与platform/DTB输出只作为ignored validation output，没有手工修改或
提交。

**Review:** 最终source/lock/lifecycle review未发现Apollyon、Keter或Euclid。两遍driver snapshot的callback只
计数或clone到已预留Vec，allocation、attach、spawn、request、abort和join均在`DriverBase.devices` guard外；
boot期device集合漂移由常开assert暴露。device attachment slot是唯一Quiescent/Active truth，physical port不持有
attachment，因此不存在`port -> attachment -> endpoint -> port`环。成功顺序为attach -> IRQ context ->
request -> slot commit -> RX enable；request成功后没有可返回失败的步骤，IRQ也不读取optional slot/notifier。
request失败路径在RX仍关闭时同步abort。

审计确认production tree中没有`RAW_SERIAL`、NS16550A raw `CharDev`/registration/minor bookkeeper、IRQ printk、
console RX、claim/lease/mode、polling watchdog或devfs TTY publication。IRQ private data直接持有port与notifier；
raw guard只覆盖fixed-ring publish/dequeue，wake发生在guard外。TX证明范围仅限port-owned lock的batch/poll上界，
不把generic console registry的既有外层IRQ-save guard宣称为本阶段证明。所有tracked source/config diff均位于
corrected C3 manifest；没有修改boot、bus、console、IRQ core、kthread/Event/wait/scheduler、apps、rootfs manifest、
test profile、current contracts或register。

**Validation:** `git diff --check`与`just fmt kernel --check`通过。xtask kconfig定向单元测试共5项通过，包含
四项TTY transport参数的零值拒绝与4096/256/16/65536 default generation。沙箱内首次`just build`仍在lwext4
C编译被seccomp以`Bad system call`阻断；沙箱外同一repository入口随后以RV64 release、KUnit、ext4和irqsave
features编译/link通过，端到端wrapper又从重建rootfs与本地测试盘副本完成一次同配置build。`mdbook build
docs`通过，只报告既有large search-index warning。

`./scripts/run-user-test-rv64.sh etc/preliminary/images/sdcard-rv.img build/tty-stage1-rv64.log`成功启动QEMU；
全量222项enabled KUnit打印`All tests passed!`。6项`device::tty` attachment/worker KUnit与4项NS16550A
FIFO/drop-new、RX budget/error和TX partial-timeout KUnit全部为`ok`。KUnit后向guest serial注入371-byte可识别
ASCII burst，日志恰有一条`TTY Stage 1 diagnostic: first RX drain on /soc/serial@10000000 accepted 64 byte(s)`；
未见panic、deadlock、recursive IRQ printk或`kthread spawn before kthreadd initialization`。达到证据点后经QEMU
monitor `quit`正常退出，wrapper返回0；已经开始的jobctl/signal/wait LTP输出及其既有成败不属于Stage 1证据。
本轮没有运行LA64 build/runtime、hardware RX、BusyBox交互TTY或Stage 2 ABI验证，RV64结果不外推到这些层级。

**Checkpoint result:** Checkpoint 3 **Closed**。driver-local quiescent/Late bridge只为当前bus缺少dependency-aware
deferred probe服务；未来generic deferred probe/retry具备`kthreadd` dependency表达后，必须删除Late initcall与
Quiescent -> Active迁移，并复用同一activation transaction，不能把本bridge扩展为generic lifecycle。

## Stage 1 closure audit - 2026-07-23

**Exit audit:** Checkpoint 1 same-owner split、Checkpoint 2 dormant attachment和Checkpoint 3 production wiring均已
独立review、验证和关闭。production NS16550A只保留fixed output console + unpublished `TtyPort`，RX/TX/identity/
applied-line owner与canonical 6.2一致；raw major 234路径为零，request失败cleanup与request后不可失败commit边界
闭合。focused KUnit、RV64 build/QEMU/RX burst、source/lock/manifest audit达到Stage 1 floor；LA64未验证按用户处置
明确保留，不冒充closure proof。

**Contract / lifecycle:** Stage 1 contract cutover为None；`TTY-DATA-CUTOVER`与`TTY-JOBCTL-CUTOVER`仍为Not Cut
Over，全部`TTY-*`仍不是current contract，register/current limitations无需变化。Stage 1现为**Closed**；Stage 2
仍为Outline且未解析、未授权、未激活。下一步只能在新的明确授权下执行独立的Stage 1 -> Stage 2 Implementation
Resolution Gate，读取live Stage 1 capability、实际diff/review/validation与current contracts后解析完整Stage 2
Ready；本次不进入Stage 2。

## Stage 1 -> Stage 2 resolution preflight - 2026-07-23

**Authorization / scope:** 用户明确授权解析Stage 2实现过程，并补充`anemone-apps/`、`anemone-rs/`、
`anemone-abi/`可作为本RFC剩余阶段的长期可写scope envelope；该授权不包括Stage 2实现。本gate保持docs-only，
三个目录仍按每个Ready stage的exact manifest取实际子集，不把scope envelope解释为无关清理许可。

**Live input:** 入口为`dev/drc/omega@c25cc816`、worktree clean。preflight读取Stage 1 production source、实际
diff/review/closure和RV64验证：fixed console + unpublished `TtyPort`、fixed raw ring、per-port kthread、bounded
TX、immutable OF identity、pre-publish abort均已接线；Stage 1 discard/first-drain sink仍是Stage 2 publish前必须
删除的唯一consumer bridge。LA64 build/runtime、hardware RX和Stage 2 ABI仍为Not Run。另审计live FileOps/
IoctlCtx、iomux Latch、devfs单向publish、console selection/anonymous stdio、post-Late boot ordering、NS16550A
applied-line/TX idle能力，以及repository app/rootfs/QEMU入口；没有发现要求改变R0 owner或generic VFS contract的
live drift。

**BusyBox evidence:** omega本地初赛盘mount现可用并保持只读。RV64 glibc artifact为BusyBox 1.33.1、RISC-V
double-float static ELF，SHA-256
`1ef4811837a8abdfe717db94d3a9c4e518233227ed0bb0cfe358b24d625664bd`；通过本机RV64 qemu-user执行确认
`ash`、`stty`、`vi` applet存在。LA64 glibc static ELF SHA-256为
`52fe6e248922e345cbd46543690a7e7c9000dc87c9d87cec3f0e7162b2f2ef94`，其applet/runtime留给LA64 wrapper在
实际使用前核对。canonical plan只记录版本/架构/hash/provenance要求；个人mount路径、BusyBox二进制和测试盘
master不进入仓库。

**Resolved decisions:** Stage 2采用Terminal-owned bounded output queue；完整`OPOST/ONLCR` transform token入队
才消费一个用户byte，echo共享同一queue，per-endpoint worker统一处理raw RX、discipline、TX与drain。driver
attachment长期持worker handle，open provider只持wake source的Weak引用，opened file可持窄strong capability；
Terminal/endpoint/port不持handle，以避免strong cycle。
termios复用新增asm-generic ABI；unsupported mask和hardware line只允许unchanged，实际改变稳定`EINVAL`。
relationless `ISIG`真实执行control-byte consume/flush/echo，但因无foreground relation不调用Signal；winsize
同理提交truth并延期`SIGWINCH` effect。endpoint按全部OF identity排序编号；console唯一拥有selected-terminal
identity，post-Late boot finalize完成prepare、single-way publish和real stdio安装。

**Checkpoint / harness result:** canonical
[Stage 2 Ready](../../rfcs/tty-subsystem/implementation.md#7-stage-2-readyterminal-data-plane-与-tty-data-cutover)
冻结四个checkpoint：Terminal discipline/worker、FileOps/UAPI/readiness、endpoint/console/boot publication、
userspace acceptance/`TTY-DATA-CUTOVER`。第四checkpoint新增独立`tty-test`、RV64/LA64 TTY rootfs与repository
wrapper；wrapper显式接收BusyBox和测试盘，只复制到`build/tty-acceptance/staging/<arch>/busybox`，不修改默认
pretest `init -> user-test -> LTP`链。自动matrix要求双架构，人工vi checklist以已核对的RV64 artifact为必要
cutover证据。

**Target / contract / validation:** 本次没有改变target、owner、ABI包络、visible semantics、accepted
limitation、cutover unit或acceptance boundary，因此R0不递增，`invariants.md`和tracking issues不变。
`TTY-DATA-CUTOVER`与`TTY-JOBCTL-CUTOVER`继续Not Cut Over，current contracts/register/current limitations
不变。本gate只执行source/config/artifact只读审计和docs write-back；未运行kernel/app build、guest QEMU、
BusyBox guest交互、LTP或hardware test，这些不得从Stage 1证据推断。

**Result:** Stage 1 -> Stage 2 Resolution Gate完成，Stage 2为**Ready / Not Started**。用户本轮授权到此；
Checkpoint 1仍未授权、未激活，不能自动进入实现。下一步若获实现授权，必须从canonical 7.5的Checkpoint 1
开始，并继续保持一个checkpoint一个activation/review/closure边界。

**Docs validation:** `git diff --check`与`mdbook build docs`通过；mdBook只报告既有large search-index warning。
定向状态/anchor/source-path审计确认implementation、RFC入口、transaction、RFC总索引和当前双周devlog一致指向
Stage 2 Ready / Not Started，新增链接命中实际HTML anchor，新增公共文本没有泄漏个人mount路径。本次没有修改
`invariants.md`、tracking issues、current contracts、register、kernel、apps、rootfs或test profile。
