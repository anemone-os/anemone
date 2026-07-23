# TTY Subsystem 目标与不变量

**状态：** Accepted Target
**最后更新：** 2026-07-23
**父 RFC：** [RFC-20260722-tty-subsystem](./index.md)
**适用修订：** R1

本文定义本 RFC 的 accepted contract delta、target invariants，以及只服务本次迁移、checkpoint 和验收的 RFC-local proof obligations。当前已经生效的共享规则仍以 `docs/src/contracts/` 中的稳定 ID 为唯一权威；`TTY-DATA-CUTOVER` 已使 `TTY-PORT-001`、`TTY-TERM-001`、`TTY-INPUT-001`、`TTY-OUTPUT-001` 与 `TTY-ENDPOINT-001` 生效，其 current truth 见 [TTY data-plane contract](../../contracts/tty/data-plane.md)。`TTY-REL-001`、`TTY-JOBCTL-001`、`TTY-LIFE-001` 与 `TTY-ABI-001` 仍只是 R1 accepted target、尚未 cut over。R1 保持 R0 的功能、owner、ABI、visible semantics 与 cutover unit，只把本 RFC 的 build/runtime acceptance evidence 收窄为 RV64。

## 规则分类

- **Correctness Invariant：** 状态唯一 owner、并发、生命周期、cleanup、内存安全和 ABI 诚实性等“违反即不正确”的规则；不得因实现成本降低为 accepted limitation。
- **Target Guarantee / Capability：** 本 RFC承诺的 serial TTY 功能与兼容包络；在形成 accepted revision 后，只能通过 `Target Renegotiation Gate` 调整。
- **Implementation Preference：** Rust 类型名、字段布局、锁类型、容器、容量、deferred carrier、TX 模式和 stage 顺序不属于 invariant，由后续 implementation resolution 根据 live source 与 probe 证据选择。

## Contract Impact

`TTY-DATA-CUTOVER` 与 `TTY-JOBCTL-CUTOVER` 是独立语义切换单元。[迁移实施计划](./implementation.md)负责按滚动gate解析具体stage、write set、验证floor、停止条件和transaction evidence；前者已经在 Stage 2 closure 生效，后者仍为 Not Cut Over。

| Contract ID | 变化 | 当前规则 | Target 摘要 | 生效 Gate |
| --- | --- | --- | --- | --- |
| `TTY-PORT-001` | Introduce | [Active](../../contracts/tty/data-plane.md#tty-port-001--物理端口与-raw-handoff-只有一个-owner) | NS16550A 固定提供 console + `TtyPort`、不注册 raw `CharDev`；port 唯一拥有硬件、TTY RX 与 boot-applied configuration | `TTY-DATA-CUTOVER`（Effective） |
| `TTY-TERM-001` | Introduce | [Active](../../contracts/tty/data-plane.md#tty-term-001--endpoint共享唯一terminal-semantic-truth) | 同一 endpoint 的 open file 共享唯一 terminal semantic truth | `TTY-DATA-CUTOVER`（Effective） |
| `TTY-INPUT-001` | Introduce | [Active](../../contracts/tty/data-plane.md#tty-input-001--input-ownershiprecord-boundary与readiness同源) | RX handoff、discipline、read 与 readiness 保持有序、有界、无 lost wake 且不复制 input truth | `TTY-DATA-CUTOVER`（Effective） |
| `TTY-OUTPUT-001` | Introduce | [Active](../../contracts/tty/data-plane.md#tty-output-001--输出按用户byte计量并由port最终序列化) | TTY 输出是 byte stream；console/TTY 共用 UART 时由 driver 唯一序列化 TX | `TTY-DATA-CUTOVER`（Effective） |
| `TTY-ENDPOINT-001` | Introduce | [Active](../../contracts/tty/data-plane.md#tty-endpoint-001--endpoint-publication是稳定的单向transaction) | serial endpoint 在完整初始化后单向发布，名称与 identity 稳定到重启 | `TTY-DATA-CUTOVER`（Effective） |
| `TTY-REL-001` | Introduce | None（尚未生效） | TTY relation owner 唯一持有 session-terminal binding 与 foreground selector | `TTY-JOBCTL-CUTOVER` |
| `TTY-JOBCTL-001` | Introduce | None（尚未生效） | terminal access 与 signal generation 通过 guards-out、identity-revalidated handoff 接入现有 topology/Signal/jobctl owner | `TTY-JOBCTL-CUTOVER` |
| `TTY-LIFE-001` | Introduce | None（尚未生效） | relation cleanup 先撤销可发现性，再完成 owner-local wake/drop；首版不生成 disassociation signal | `TTY-JOBCTL-CUTOVER` |
| `TTY-ABI-001` | Introduce | None（尚未生效） | 真实交付 BusyBox ash/vi 所需的 `/dev/tty*`、termios、iomux 与常用 foreground job-control 包络 | `TTY-JOBCTL-CUTOVER` |
| [`SIGNAL-PENDING-001/002`](../../contracts/signal/pending-routing.md) | Preserve | task-private / ThreadGroup-shared pending各有唯一owner，group-directed publication与member notification分离 | terminal-generated ordinary occurrence只通过现有Signal pending owner发布；TTY不保存occurrence或以wake结果驱动policy | 全程 |
| [`SIGNAL-ACTION-001/002`](../../contracts/signal/pending-routing.md) | Preserve | live disposition拥有ignored admission与ordinary action selection；条件stop只在DefaultStop选择后进入jobctl | TTY只请求Signal owner判断blocked/ignored/actionable并生成occurrence，不自行提交ignore、handler、default-stop或restart后的action | 全程 |
| [`PGRP-SIGNAL-001/002`](../../contracts/task/process-group-signaling.md) | Preserve | ProcessGroup 只选择成员，每个 ThreadGroup 独立接受 occurrence | TTY 只提交准确的 process-group signal request，不取得 membership 或 signal truth | 全程 |
| [`JOBCTL-STATE-001`](../../contracts/task/job-control.md#jobctl-state-001--threadgroup持有唯一job-control-truth)、[`JOBCTL-SIGNAL-001`](../../contracts/task/job-control.md#jobctl-signal-001--control-signal-generation与jobctl提交同序)、[`JOBCTL-LIFE-001`](../../contracts/task/job-control.md#jobctl-life-001--membership与terminal不遗留exposure或parker) | Preserve | ThreadGroup 唯一拥有 stop/continue、control ordering、exposure、report 与 jobctl cleanup | TTY 不设置 stopped bit、不重放 resume、不从 wait/report 反推 foreground truth | 全程 |
| [`TASK-LIFE-001..003`](../../contracts/task/thread-group-lifecycle.md) | Preserve | ThreadGroup lifecycle 唯一拥有 terminal code、Exited publication 与 parent notification ordering | TTY relation cleanup 不覆盖 first terminal code，也不复制 lifecycle truth | 全程 |
| [`USER-ENTRY-002`](../../contracts/task/user-entry.md#user-entry-002--所有user-transition共享mandatory-gate) | Preserve | 所有 user transition 服从现有 Signal/lifecycle/jobctl mandatory gate | terminal signal 最终效果仍由现有 Signal 与 ThreadGroup jobctl 路径闭合 | 全程 |

### Prospective cutover 边界

- `TTY-DATA-CUTOVER` 只允许在 port、terminal、input/output、endpoint publication 和 data-plane ABI 已形成可独立使用且 ABI 诚实的能力后切换对应 contract。它可以先于完整 job control，但不能关闭本 RFC，也不能宣称 interactive shell 已取得 controlling TTY。
- `TTY-JOBCTL-CUTOVER` 必须在 relation、terminal access、signal handoff、cleanup 和首版完整验收包络作为一个自洽单元验证后切换。它不能在仅有 shell prompt、匿名 console 兼容桩或 `job control turned off` 降级模式下发生。
- R1 的两个 cutover 都只使用 RV64 build/runtime、自动 matrix 与对应人工 checklist 作为目标架构 proof；LA64 compile/runtime 为 Not Run，不是 cutover blocker，也不能由 RV64 结果推断为已验证。source/KUnit 中与架构无关的规则仍必须逐项闭合，不能用 RV64-only disposition 缩小语义 matrix。
- 任一 cutover 失败时，对应 `Introduce` ID 保持 Not Cut Over；不得把 probe、部分实现或 Draft RFC 写成 effective contract。公开 RFC review 可以在保持 target 的前提下调整具体 stage，但若拆分会改变 owner、ABI、target guarantee 或接受边界，必须回到 target review。

## Target Invariants

### TTY-PORT-001 — 物理端口与 raw handoff 只有一个 owner

**类别：** Correctness Invariant。

**规则：** UART driver 或其 driver-local frontend 唯一拥有 MMIO、IRQ、硬件 FIFO、boot-applied line configuration、line-error observation，以及 IRQ 到 deferred consumer 之间的 raw RX handoff。TTY core 只持有窄 `TtyPort` capability，不复制 register、FIFO、applied configuration 或 raw queue truth。

第一版 NS16550A 从同一 physical owner 固定同时提供 output-only console backend 与 `TtyPort` capability，不实现或注册 raw `CharDev`。两者不是互斥 personality：console 不消费 RX、不拥有运行时线路配置；boot-applied configuration 在 capability publication 前由 driver 提交并在运行期保持不变；`TtyPort` 的 pre-publish consumer binding 是 UART RX 的唯一去向。注册、console enable、TTY open/close都不切换整体模式，driver不得为这组固定关系建立claim、lease或mode state。

raw handoff 必须有明确容量上限、保持接收顺序，并对 overflow、line error 和分配失败提供可审计 counter/策略。hard IRQ 只允许 bounded drain、raw publication、统计和窄 deferred notification；不得 sleep、user copy、执行 line discipline、生成 signal、唤醒 user waiter、递归输出到同一 UART、执行复杂 drop，或进入 task topology/Signal owner。若实现选择 IRQ/noirq 下的 fallible growth，失败不得触发递归 OOM side effect，只能进入同一有界丢弃与观测边界。

console 与 TTY 共用同一 UART 时，所有普通 TX 的最终 serialization 也由该物理 driver 唯一拥有且必须 IRQ-safe；TTY 不能用一把只覆盖自身的锁伪造全端口互斥。该 serialization 可以用一把 driver-owned lock 或等价单一 queue owner 实现，但不能把任意长度用户 write 变成无界 IRQ-off 临界区。持有 serialization 的路径不得递归 printk，RX IRQ 也不得打印回同一 UART。

**Owner：** UART physical driver / driver-local `TtyPort` frontend。

**依赖：** None；本条定义后续 terminal input/output handoff 使用的 physical-port baseline。

**违反表现：** RX IRQ 与 TTY 各持一份队列状态、NS16550A 仍注册 raw `CharDev`、console 与 TTY 同时消费 RX、console/open/close 驱动 personality 切换、首版保存 claim/lease/mode state、line discipline 在 hard IRQ 执行、overflow 静默覆盖旧数据、TTY 直接访问 UART register，或 console/TTY TX 发生无 owner 的交错。

**Cutover：** `TTY-DATA-CUTOVER`。

### TTY-TERM-001 — Endpoint 共享唯一 terminal semantic truth

**类别：** Correctness Invariant。

**规则：** 同一 serial endpoint 的所有 open file 引用同一个 terminal semantic owner。该 owner 唯一持有 committed termios、winsize、concrete N_TTY-like discipline、canonical pending edit、committed/noncanonical input、readiness predicate，以及未来进入兼容包络后的 hangup truth。opened file 只能保存 terminal reference 和确有语义的 per-open state；termios、winsize、input、foreground selector、hangup 与 `O_NONBLOCK` 不得复制为 per-file truth，`O_NONBLOCK` 每次从通用 open-file-description ctx 读取。

用户可见属性更新只能在整个 semantic snapshot 可提交时发布。真实无效、不可表示或需要未支持硬件行为的请求必须失败并保持旧 snapshot；行为等价的兼容位可以稳定 round-trip、归一化或静默兼容，但必须按正文策略保留限频诊断。不得先报告成功或发布新 snapshot，再让 backend 悄悄维持冲突状态。

**Owner：** `device::tty` terminal semantic owner。

**依赖：** `TTY-PORT-001`。

**违反表现：** 不同 fd 观察互相冲突的 termios/input、file 缓存 stale nonblock bit、port shadow 反向驱动 terminal semantics、失败 update 部分可见，或 ioctl success stub 丢弃用户状态。

**Cutover：** `TTY-DATA-CUTOVER`。

### TTY-INPUT-001 — Input ownership、record boundary 与 readiness 同源

**类别：** Correctness Invariant。

**规则：** raw handoff dequeue 是 byte record 从 port owner 转入 deferred consumer 的唯一 ownership transfer；consumer 按序把有界 batch 交给 terminal discipline，提交后不得保留可重放副本。notification 只请求重验，raw-handoff nonempty predicate 才是 durable work truth；Terminal 的 committed input predicate同时驱动 blocking read 与 poll/select readiness，硬件 FIFO、worker wake 和 Event payload都不得成为第二份 readable truth。

canonical mode 必须区分 pending edit 与 committed record：半行不可读，delimiter/`VEOF` 按已承诺语义提交，一次 read 不越过 canonical boundary，`VERASE`/`VKILL` 不修改已提交 record。noncanonical `VMIN=1, VTIME=0` 必须提供真实 byte-stream input。blocking waiter和 deferred consumer 都使用 predicate publication + recheck，覆盖 notification-before-publication、notification-after-publication 与 concurrent drain 三种窗口。

一次 read 只能消费本次选择的 prefix。显式 flush、已记录 overflow policy和正文接受的 post-validation user-copy fault边界可以丢弃已经取得所有权的数据；普通成功路径不得重复消费、凭空产生、越界消费或为追求 rollback 建立第二份 input truth。

**Owner：** raw bytes 在 transfer 前归 `TtyPort` handoff；提交后的 editable/readable input 归 terminal discipline。deferred consumer只在两者之间临时持有已 dequeue 的 batch，不是持久状态 owner。

**依赖：** `TTY-PORT-001`、`TTY-TERM-001`。

**违反表现：** lost work/lost wake、半行使 poll readable、一次 read 跨越 record、worker wake携带 durable input、copy fault回滚制造重复 byte，或 queue overflow无诊断覆盖旧输入。

**Cutover：** `TTY-DATA-CUTOVER`。

### TTY-OUTPUT-001 — 输出按用户 byte 计量且由 port 最终序列化

**类别：** Correctness Invariant。

**规则：** TTY write 接受任意 bytes，不继承 anonymous console 的 UTF-8 校验。关闭 `OPOST` 时按原 bytes 提交；启用输出变换时，partial progress 仍按已经消费的用户输入 bytes 计量，单个输入 byte 的扩展不得造成重复提交。echo 使用同一 terminal output transform 和 port TX capability，但不得在 terminal guard 内等待硬件。

TX polling、bounded buffer 或 TX-empty IRQ 是实现选择；无论选择哪一种，writable、drain、partial write 与 `TCSETSW` 都必须来自实际 backend 能力。普通 console record 与任意长度 TTY write 不承诺整次原子性或优先级；实现只需以有界 batch或唯一TX queue保持寄存器/queue owner与byte progress正确。panic/early-console 可以有明确 best-effort 降级，但不能成为普通 TTY TX truth，也不能反向要求普通路径引入 personality state。

**Owner：** terminal owner 持有 output transform/progress；physical driver 持有 TX queue/register 与 console/TTY serialization。两边不共享可变真相，handoff 以已转换 batch和明确 backend progress为边界。

**依赖：** `TTY-PORT-001`、`TTY-TERM-001`。

**违反表现：** 任意 byte 因 UTF-8 失败、ONLCR partial write重发输入、echo持 terminal guard等待TX、TTY与console绕过同一serialization，或虚构不存在的drain completion。

**Cutover：** `TTY-DATA-CUTOVER`。

### TTY-ENDPOINT-001 — Endpoint publication 是稳定的单向 transaction

**类别：** Correctness Invariant。

**规则：** 每个启动期成功注册的 TTY-capable serial port 获得不可变 identity 和确定性的逻辑实例号；编号不得依赖并发 probe 的偶然完成顺序。publish 前必须完成 identity 唯一性校验、Terminal/raw-handoff 初始化、deferred consumer binding 和 open provider构造；失败只回滚尚不可见的本地对象。devfs publish 是可见线性化点，成功后 `/dev/ttyS<N>` 名称、device number、endpoint identity和共享 Terminal保持到重启。

第一版不支持已发布 endpoint 的 runtime unpublish、重新编号或编号复用。最后一个 fd close、controlling relation detach或session exit都不删除 endpoint。`/dev/console` 的节点与选择 truth继续属于 console owner；boot fd可以消费其 selected-terminal identity，但不能据此把 console registry或publication移入TTY。

**Owner：** TTY endpoint registry / devfs publication protocol；port identity来源仍归platform/driver owner，console identity来源仍归console owner。

**依赖：** `TTY-PORT-001`、`TTY-TERM-001`。

**违反表现：** node先于consumer可用、失败后留下半发布endpoint、重启前名称漂移/复用、最后close删除Terminal，或TTY接管`/dev/console` truth。

**Cutover：** `TTY-DATA-CUTOVER`。

### TTY-REL-001 — Controlling-terminal relation 是单一双向 binding truth

**类别：** Correctness Invariant。

**规则：** TTY relation owner 中的一个 relation 唯一持有 terminal identity、stable session identity、foreground process-group identity和用于 stale detection 的 generation/等价身份。Session/SID lookup 与 terminal lookup可以持有指向同一 relation 的 handle，但不得各缓存一份可变 binding或foreground PGID。每个 session至多一个 controlling terminal，每个 terminal至多一个 controlling session。

task topology继续唯一拥有Session/ProcessGroup membership。TTY只能通过窄查询/decision capability验证caller、session leader和候选foreground group；裸SID/PGID只用于ABI lookup，跨lifecycle保存的relation和foreground target必须绑定stable identity并在owner边界重验，ID reuse不得复活旧控制权。

`/dev/tty` open按同步caller的stable session identity解析live relation，成功后返回正常Terminal file；后续I/O不能用最近reader/opener或全局PGID重新猜测terminal。foreground mutation必须先由topology验证candidate，再回到relation owner重验relation generation与caller authority并提交，不能用跨owner stale snapshot直接写入。

**Owner：** TTY controlling-terminal relation registry。

**依赖：** task topology 的Session/ProcessGroup identity与membership owner；不改变其current contract。

**违反表现：** Session与Terminal各存一份foreground PGID、numeric ID reuse取得旧relation、`TIOCSPGRP`只检查正整数、non-controlling caller通过`/dev/tty`取得任意Terminal，或opener identity驱动后续access policy。

**Cutover：** `TTY-JOBCTL-CUTOVER`。

### TTY-JOBCTL-001 — Terminal policy 只产生经重验的 guards-out effect

**类别：** Correctness Invariant。

**规则：** TTY拥有terminal-side foreground/background access policy与terminal-signal request generation；task topology拥有caller/group/session membership，Signal拥有occurrence，ThreadGroup jobctl拥有stop/continue phase、control ordering、user-entry gate与parent report。TTY不得跨越这些owner直接设置stopped/continued state、完成ordinary wait、修改report，或从`jobs`/wait结果反推foreground selector。

每次read/write/ioctl以同步current caller为准。TTY先在relation owner内取得带stable identities的immutable decision snapshot，释放TTY guard后进入topology/Signal owner重验caller与target membership。`TIOCSPGRP`还必须在该decision中读取current caller的signal mask和共享disposition：foreground或background + blocked/ignored `SIGTTOU`可以继续，background + actionable `SIGTTOU`必须先向caller process group生成signal并返回restart，不得提交relation mutation。只读access或signal effect可以线性化为relation snapshot对应的合法前态；relation mutation必须在topology验证后返回relation owner重验generation再commit。任何路径都不得把完整Task、topology lock、Signal state或scheduler state长期存入Terminal。

terminal guard、port guard与topology guard外才允许发布Signal、Event wake、echo TX和复杂drop。foreground control characters、background read与winsize change只能作用于snapshot指定且经membership重验的foreground process group；不存在合法target时必须retry、fail-close或走已记录unsupported边界，不得回退到current task、最近reader或全局PGID。

**Owner：** TTY terminal-access protocol拥有decision generation；各参与方只提交自己的local state，现有`SIGNAL-PENDING-*`、`SIGNAL-ACTION-*`、`PGRP-SIGNAL-*`、`JOBCTL-*`与`USER-ENTRY-*`owner保持不变。

**依赖：** `TTY-REL-001`、`SIGNAL-PENDING-001/002`、`SIGNAL-ACTION-001/002`、`PGRP-SIGNAL-001/002`、`JOBCTL-STATE-001`、`JOBCTL-SIGNAL-001`、`USER-ENTRY-002`。

**违反表现：** session外group收到terminal signal、TTY保存或推进jobctl phase、background policy使用opener/global PGID、持TTY lock进入Signal/topology导致lock inversion，或signal delivery结果反向改写relation。

**Cutover：** `TTY-JOBCTL-CUTOVER`。

### TTY-LIFE-001 — Relation cleanup 先撤销可发现性再执行外部效果

**类别：** Correctness Invariant。

**规则：** session leader/controlling process exit与首版session-leader `TIOCNOTTY`终结controlling relation。cleanup由relation owner唯一提交且幂等：先使旧relation不再能被`/dev/tty`、foreground check或后续mutation取得，再释放guard并执行必要的owner-local wake/drop。并发access只能观察可解释的旧前态或已撤销后态，不能观察无owner的half-detached relation。

首版cleanup只撤销relation与foreground selector，不依据旧foreground snapshot生成`SIGHUP`/`SIGCONT`。这是一项明确的scoped limitation，不把signal effect转交给topology/jobctl，也不与hardware hangup或newly orphaned stopped process-group effects混为同一状态转换。后续若要增加relation-disassociation signals，必须先由target review定义触发条件、signal target、owner-local revalidation、验证oracle与register disposition。

foreground process group消失只清理/失效foreground selector，不自动拆除session-terminal relation。最后一个ordinary fd close同样不拆 relation。TTY cleanup不得覆盖`ThreadGroupLifeCycle`的first terminal code或jobctl terminal precedence；task topology cleanup不得重建termios/input/relation truth。newly orphaned stopped process-group detection继续属于topology/jobctl，即使后续effect与terminal并发也不能迁入TTY。

已发布serial endpoint、devfs node和Terminal不因relation cleanup销毁。hardware hangup/backend fatal lifecycle在进入后续target revision前，不得通过节点消失、编号复用或销毁Terminal伪装实现。

**Owner：** TTY relation owner；ThreadGroup terminal lifecycle与orphan transition仍由现有task/jobctl owner持有。

**依赖：** `TTY-REL-001`、`TTY-JOBCTL-001`、`TASK-LIFE-001..003`、`JOBCTL-LIFE-001`。

**违反表现：** detach后`/dev/tty`仍取得旧relation、两个owner重复发送cleanup effects、TTY覆盖first exit code、foreground group消失误删endpoint，或last-close销毁仍受session控制的Terminal。

**Cutover：** `TTY-JOBCTL-CUTOVER`。

### TTY-ABI-001 — 首版兼容包络必须真实可观察

**类别：** Target Guarantee / Capability。

**规则：** 首个完整target必须同时交付：稳定`/dev/ttyS0`与`/dev/tty`open语义；boot fd 0/1/2指向策略选定的真实Terminal；canonical与noncanonical `VMIN=1,VTIME=0` input；blocking/nonblocking read、byte-stream write与poll/select readiness；正文列出的termios flags、control chars、winsize与ioctl下限；显式`setsid()` + `TIOCSCTTY(arg=0)` acquisition；`TIOCGPGRP/TIOCSPGRP/TIOCGSID`；foreground `VINTR/VQUIT/VSUSP`、winsize `SIGWINCH`、普通background read `SIGTTIN`，以及只撤销relation和foreground selector的session leader detach/exit cleanup。`TIOCSPGRP`的首版非orphan语义必须区分foreground allow、background + blocked/ignored `SIGTTOU` allow和background + actionable `SIGTTOU` signal-and-restart，只有decision允许且relation generation重验成功后才能提交foreground mutation。

BusyBox ash必须取得controlling TTY，且`jobs`、Ctrl-Z、`fg`、`bg`、foreground Ctrl-C、ordinary background read，以及foreground job结束或停止后shell通过ignored/blocked `SIGTTOU`收回terminal的`TIOCSPGRP`路径走真实relation/Signal/jobctl decision；BusyBox vi必须能够依靠真实raw/canonical切换、readiness和byte I/O完成启动、编辑、保存与退出。仅显示shell prompt、打印`job control turned off`、无条件放行`TIOCSPGRP`、匿名console特判或ioctl success stub都不满足本target。

包络内语义必须真实执行。session-leader detach/exit的`SIGHUP`/`SIGCONT`必须登记为scoped limitation；其它包络外能力，包括PTY、runtime hardware line reconfiguration、hardware hangup/backend fatal lifecycle、`TIOCSCTTY(arg=1)`steal、non-leader局部`TIOCNOTTY`、`TOSTOP` write、非`TIOCSPGRP` terminal-modifying operations的`SIGTTOU` matrix、`TIOCSPGRP` orphaned-pgrp errno和orphaned-pgrp effect，可以稳定拒绝或登记对应limitation。不得把`TIOCSPGRP`的foreground或background blocked/ignored/actionable非orphan路径归入该延期范围，不得返回成功后丢弃状态，也不得借fallback target伪造支持。

**Owner：** TTY ABI surface；具体signal/jobctl结果继续由`TTY-JOBCTL-001`列出的participant owner提交。

**依赖：** `TTY-TERM-001`、`TTY-INPUT-001`、`TTY-OUTPUT-001`、`TTY-ENDPOINT-001`、`TTY-REL-001`、`TTY-JOBCTL-001`、`TTY-LIFE-001`。

**违反表现：** ash只能降级运行、foreground job结束后shell无法收回terminal、`TIOCSPGRP`无条件放行或错误fail-close其blocked/ignored路径、vi依赖fake ioctl、unsupported设置成功但无效果、background read不经过foreground policy，或data-plane checkpoint被写成完整RFC closure。

**Cutover：** `TTY-JOBCTL-CUTOVER`。

## RFC-local Invariants

### TTY-LOCAL-001 — Data-plane checkpoint 不等于完整 target closure

serial RX、canonical/raw、echo、read/write/poll和BusyBox vi可以先形成独立checkpoint，也可以在满足证据后执行`TTY-DATA-CUTOVER`。该checkpoint不得把`TTY-REL-*`、`TTY-JOBCTL-*`、`TTY-LIFE-*`或`TTY-ABI-001`写成effective，不得关闭RFC，也不得把`can't access tty; job control turned off`登记为本RFC的最终accepted limitation。

**删除/退出条件：** `TTY-JOBCTL-CUTOVER`完成后，本条作为历史proof obligation保留，不进入current contract。

### TTY-LOCAL-002 — 两个 cutover 都必须原子且有 transaction evidence

每个prospective cutover必须在同一文档/代码收口中记录对应ID、live owner审计、验证来源、失败项与Not Cut Over结果；不得让同一cutover unit出现部分ID已经Active、其余仍以Draft RFC为authority的状态。后续若implementation resolution需要重新划分cutover unit，必须先证明target与跨owner handoff仍自洽，并在公开RFC review中更新本页，不能由执行stage自行拆分。

**删除/退出条件：** 两个cutover均完成或明确Not Cut Over，并由transaction与current contract记录最终结果。

### TTY-LOCAL-003 — 迁移桥不得成为第二条长期正确性路径

现有anonymous console stdin、NS16550A raw serial major 234 `CharDev` registration、bring-up polling timer或临时debug injection只能作为迁移输入或可删除probe。`TTY-DATA-CUTOVER`前必须完成boot fd替换并删除NS16550A的raw `CharDev`实现/注册；固定console + `TtyPort`关系必须证明TTY是唯一RX consumer、console不参与线路配置且普通TX经过同一port-owned IRQ-safe serialization。周期poll最多用于定位notification问题，必须带诊断和删除条件，不能补偿lost-work路径后留在正式正确性证明中。

**删除/退出条件：** 对每个bridge记录remove或follow-up RFC结论；raw `CharDev`、claim/lease/mode state与没有明确归属的bridge不得越过`TTY-DATA-CUTOVER`，output-only console按`TTY-PORT-001`作为正式固定projection保留。

### TTY-LOCAL-004 — Probe 选择不自动冻结长期抽象

预分配ring与bounded fallible growth、专用RX kthread与其它deferred carrier、polling与IRQ TX、粗粒度guard与更细拆分都可以通过probe选择。probe必须声明hypothesis、最小write set、失败信号、观测counter和删除/回写条件；偶然跑通不得把worker、queue、generic workqueue、transport framework或public trait自然沉淀为target。若证据要求改变owner、ABI、lifecycle、cutover或完整验收包络，必须停止并进入target review。

**删除/退出条件：** 对应implementation gate以source/runtime证据选定实现并删除probe-only surface，或显式触发Target Renegotiation Gate。

### TTY-LOCAL-005 — 验收证据必须分层且诚实

data-plane proof至少分别覆盖IRQ/raw handoff与overflow观测、canonical/noncanonical input、blocking/nonblocking/readiness、termios/control chars、byte output和BusyBox vi；job-control proof至少覆盖controlling acquisition、`/dev/tty`、foreground切换、Ctrl-C/Ctrl-Z、`jobs/fg/bg`、ordinary background read和relation撤销cleanup。首版cleanup proof只要求detach/exit后旧relation、`/dev/tty`与foreground selector不可再取得，不把`SIGHUP`/`SIGCONT`列为通过条件。ash验收至少包含`Ctrl-Z -> jobs -> fg -> Ctrl-Z -> bg -> fg`一类能重复交接并收回foreground PGID的序列，且定向`TIOCSPGRP` proof分别触达foreground allow、background + blocked/ignored `SIGTTOU` allow和background + actionable `SIGTTOU` signal-and-restart；orphaned-pgrp errno可以明确标为延期。shell prompt只证明程序启动，不证明TTY、controlling relation或job control；静态source audit、KUnit、QEMU smoke和用户运行证据必须分别标注，不能互相冒充。

当前没有oracle的延期corner只能证明“未进入首版兼容包络且ABI诚实”，不能证明Linux/POSIX完整兼容。ash源码与上述交互/定向序列已经为`TIOCSPGRP`非orphan核心语义提供oracle，因此不得再以“完整`SIGTTOU` matrix待验证”为由跳过或fail-close这些路径。新增成功桩、静默fallback或未登记deviation不得作为TCONF/unsupported证据。

R1 的分层证据以 RV64 为唯一 build/runtime acceptance architecture；LA64 明确记为 Not Run。该处置只改变 proof architecture scope，不删除任何 data-plane、relation、job-control、cleanup 或 BusyBox case，也不允许把 source/KUnit/agent-run/user-run 层级互相替代。

**删除/退出条件：** transaction对每个target ID给出验证provenance与未验证边界，review确认其足以支持对应cutover结论。

### TTY-LOCAL-006 — Register 只按实际关闭的缺口收窄

`TTY-JOBCTL-CUTOVER`必须回写[`ANE-20260527-PROCESS-GROUP-SESSION-STAGE1`](../../register/current-limitations.md#ane-20260527-process-group-session-stage1)，但本RFC延后的relation-disassociation `SIGHUP`/`SIGCONT`与newly orphaned stopped process-group policy仍保持Active；不得因controlling TTY与foreground主路径完成而关闭整条limitation。应把已验证能力折回Summary/证据，并将剩余relation-disassociation signals、orphaned-pgrp、`TOSTOP`/其它terminal-modifying background-access corner或其它未完成项收窄为自洽的residual entry，不能把已经属于cutover proof的`TIOCSPGRP`非orphan核心路径重新登记为限制；必要时拆分稳定owner不同的条目。

基础serial TTY ioctl完成后，只能按实际case结果收窄[`ANE-20260604-IOCTL-LTP-STAGE1-GAPS`](../../register/current-limitations.md#ane-20260604-ioctl-ltp-stage1-gaps)中的tty子域；PTY/devpts/ptmx与runner wrapper未完成时该总条目不能据此关闭。本RFC不交付`/proc/<tgid>/stat`的tty/foreground字段，因此[`ANE-20260529-PROC-TGID-STAT-STAGE1`](../../register/current-limitations.md#ane-20260529-proc-tgid-stat-stage1)保持不变，除非后续target review显式扩展范围并补oracle。

**删除/退出条件：** 对应cutover transaction记录每个register entry的Closed、Narrowed、Split或Unchanged结论及runtime/source证据；不得以RFC target文本代替实际回写。

## 非目标

本页不新增正文范围。PTY、完整Linux termios/TTY内部模型、runtime hotplug、hardware hangup、runtime line reconfiguration和当前延期job-control corner仍以[父RFC的非目标](./index.md#非目标)与[接受边界](./index.md#接受边界)为准。类型名、模块文件、锁实现、容器与stage顺序不得因为在本页举例而升级为target invariant。
