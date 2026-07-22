# 2026-07-23 - TTY Subsystem

**Status:** Active / Stage 0 Closed / Stage 1 Ready
**Owners:** doruche, Codex
**Area:** device / TTY / serial / VFS / signal / task topology / job control
**Canonical Plan:** [RFC-20260722-tty-subsystem](../../rfcs/tty-subsystem/index.md), [目标与不变量](../../rfcs/tty-subsystem/invariants.md), [迁移实施计划](../../rfcs/tty-subsystem/implementation.md)
**Canonical Revision:** R0
**Current Phase:** Stage 0 Closed / Stage 1 Ready / Not Started

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
