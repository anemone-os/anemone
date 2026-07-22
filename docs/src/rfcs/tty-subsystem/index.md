# RFC-20260722-tty-subsystem

**状态：** Accepted for Implementation
**修订：** R0
**负责人：** doruche, Codex
**最后更新：** 2026-07-23
**领域：** device / TTY / serial / VFS / signal / task topology / job control
**事务日志：** [2026-07-23 - TTY Subsystem](../../devlog/transactions/2026-07-23-tty-subsystem.md)
**影响契约：** [目标与不变量](./invariants.md)接受引入 `TTY-PORT-001`、`TTY-TERM-001`、`TTY-INPUT-001`、`TTY-OUTPUT-001`、`TTY-ENDPOINT-001`、`TTY-REL-001`、`TTY-JOBCTL-001`、`TTY-LIFE-001` 与 `TTY-ABI-001` 的 R0 target，并 Preserve 现有 Signal、process-group、job-control、task-lifecycle 与 user-entry contract；R0 接受不改变 current contract，全部 `TTY-*` 仍为 Not Cut Over。
**开放问题：** None；已关闭的设计 finding 及重新打开条件见 [Tracking Issues](./tracking-issues.md)。
**下一步：** [Stage 1](./implementation.md#6-stage-1-readyunpublished-transport-vertical-slice) 已Active；Checkpoint 1/2关闭，Checkpoint 3按获准的driver-local quiescent probe / Late activation路线执行。Stage 1完成后不得自动进入Stage 2。

## 摘要

本 RFC 提议在 `device::tty` 建立 Anemone 第一版 TTY 子系统。每个启动期选择 TTY frontend 的 serial port 通过窄 `TtyPort` capability 注册一个共享 `Terminal`，由 TTY 发布稳定的 `/dev/ttyS<N>` 和 caller-relative `/dev/tty`，并拥有 termios、line discipline、readiness、controlling-terminal relation 与 terminal-side foreground/background policy。

第一版只交付 serial TTY，但完成目标同时包括 BusyBox `ash` 的 controlling TTY 与常用 foreground job-control 主路径，以及 BusyBox `vi` 所需的数据面。NS16550A 固定同时提供 output-only console backend 与 `TtyPort`，TTY 独占 RX；console 与 TTY 的普通 TX 由 physical port 统一序列化。现有 Unix job control 继续唯一拥有 ThreadGroup stop/continue truth。

## 背景

当前 boot stdin 是永久 EOF 的匿名 console，NS16550A RX IRQ 读取后丢弃输入；通用 `CharDev` 也没有 TTY 所需的 caller、nonblock、poll 和专属 open 接线。现有 [Unix job control contract](../../contracts/task/job-control.md)已提供 stop/continue、process-group signal selection、parent report 与 user-entry barrier，但明确不覆盖 controlling TTY、foreground/background process group 和 terminal-generated signals。

TTY 因而不能通过扩展匿名 console、在通用 `CharDev` 中加入 task/session 特判，或保存全局 foreground PGID 来补齐。详细现状与参考实现调查保存在[背景材料](./backgrounds/index.md)；它们只提供证据，不覆盖本 RFC target。

## 目标

- 在 `device::tty` 建立专属 registry、`Terminal`、FileOps、devfs publication 和 controlling relation。
- 以窄 `TtyPort` 隔离 UART 硬件；hard IRQ 只做有界 RX drain、raw handoff、统计和 deferred notification。
- 让 NS16550A 固定同时提供 output-only console backend 与 `TtyPort`，停止注册 raw serial `CharDev`；TTY 独占 RX，普通 console/TTY TX 进入同一 port-owned IRQ-safe serialization。
- 提供 canonical/noncanonical input、echo、控制字符、byte-stream output、blocking/nonblocking I/O 与 iomux readiness。
- 提供 BusyBox `ash`、`stty` 和 BusyBox `vi` 所需的 termios、winsize 和 ioctl 下限。
- 为每个启动期注册的 port 稳定发布 `/dev/ttyS<N>`，至少交付 `/dev/ttyS0`；发布 `/dev/tty`，并让 boot fd 0/1/2 引用选定的真实 Terminal file。
- 建立每个 session 至多一个 controlling terminal、每个 terminal 至多一个 controlling session 的唯一 relation，并在其中持有 foreground process-group identity。
- 通过现有 Session/ProcessGroup/Signal/ThreadGroup jobctl owner 闭合 foreground control characters、普通 background read、ash 所需的 `TIOCSPGRP` 三分支和 relation cleanup。
- 对 RX overflow、line error、unsupported termios 与延期 ABI 保持稳定结果和可观测性，不用 success stub 或 fallback target 伪造支持。

## 非目标

- 第一版不实现 PTY、`ptmx`、`devpts`、terminal multiplexer 或图形终端。
- 不支持 serial endpoint 的 runtime hotplug、unpublish、重新编号或编号复用；成功发布后保持到重启。
- 不建立可插拔 line-discipline framework，也不复制 Linux 的 `tty_struct`、flip buffer、细粒度锁或竞态胜负。
- 不把 TTY 并入通用 `CharDev`，不扩大所有字符设备对 task/session 的观察面。
- 不在 TTY、Session、Signal、ProcessGroup 或 scheduler 中复制 ThreadGroup stop/continue truth。
- 不建立 console/TTY personality manager、claim、lease 或 open/close mode switch；不让 TTY 接管 console registry、printk fan-out、selected-console truth 或 `/dev/console`。
- 不要求 `/dev/console` reopen 返回 Terminal file；boot selected-terminal identity 只用于安装真实 boot stdio。
- 不覆盖完整 Linux termios、全部 `VMIN/VTIME`、runtime baud/data/parity/stop-bit reconfiguration、modem/flow control 或 break。
- 不覆盖 hardware hangup/backend fatal lifecycle、`TOSTOP` write、其它 terminal-modifying `SIGTTOU` matrix、`TIOCSPGRP` orphaned-pgrp errno 与 orphaned-process-group effect。
- `TIOCNOTTY` 第一版只允许 session leader 撤销整个 relation；`TIOCSCTTY(arg=1)` privileged steal 返回 `EPERM`。
- session-leader detach/exit 只撤销 relation 与 foreground selector，不生成 relation-disassociation `SIGHUP`/`SIGCONT`。
- 本 RFC 不交付 `/proc/<tgid>/stat` 的 tty/foreground 字段，也不以 GNU Vim 或完整 POSIX corner matrix 为验收目标。

## 文档地图

RFC target：

- [目标与不变量](./invariants.md)：Contract Impact、target invariants、cutover unit 与 RFC-local proof obligations。
- [迁移实施计划](./implementation.md)：滚动 stage、probe、验证、停止条件和 resolved write set。
- [Tracking Issues](./tracking-issues.md)：已 neutralize 的 design finding 及重新打开条件。

Current contracts：

- [Signal pending/action](../../contracts/signal/pending-routing.md)
- [Process-group signal targeting](../../contracts/task/process-group-signaling.md)
- [Unix job control](../../contracts/task/job-control.md)
- [ThreadGroup lifecycle](../../contracts/task/thread-group-lifecycle.md)
- [User entry](../../contracts/task/user-entry.md)

背景材料：

- [背景材料索引](./backgrounds/index.md)

## 修订记录

| 修订 | 日期 | 状态 | 摘要 | 证据 |
| --- | --- | --- | --- | --- |
| R0 | 2026-07-23 | Accepted for Implementation | 接受 serial TTY owner、ABI 包络、两个 cutover unit 与 proof obligations；全部 `TTY-*` 保持 Not Cut Over | [事务日志](../../devlog/transactions/2026-07-23-tty-subsystem.md) |

## 兼容与工程原则

兼容目标是用户可观察的 TTY ABI，不是 Linux 内部对象。首版包络由 BusyBox `ash`、`stty`、BusyBox `vi` 和直接依赖的 `/dev/tty*` ABI 定义：包络内语义必须真实执行；行为等价的兼容位可以稳定 round-trip、归一化或静默忽略并保留限频诊断；包络外语义只能稳定拒绝或登记 scoped limitation。

内部可以使用粗粒度 terminal serialization、heap-backed container、同步 polling TX，以及由 implementation gate 选择的预分配 ring 或受限 noirq allocation。无论具体形状如何，都不能制造第二份 input、termios、foreground、relation 或 job-control truth，不能 lost wake、向 session 外 group 发送 terminal signal、静默覆盖 overflow，或持 terminal/port guard 进入 Signal、Event、task topology 和 sleepable TX。

## 方案

### 1. 首个交付边界分成 checkpoint 与完成目标

数据面可以先形成独立 checkpoint：UART 输入、canonical/raw、echo、read/write/poll 和 BusyBox vi 可用。它用于验证 transport、VFS 和 termios，但不能关闭 RFC。

完整目标还必须让 interactive `ash` 取得 controlling TTY，完成 foreground PGID 交接、Ctrl-C/Ctrl-Z、`jobs/fg/bg`、普通 background read 与 relation cleanup。shell prompt 或 `job control turned off` 只证明进程启动，不是 TTY closure。

### 2. 内部对象与唯一 owner

#### `TtyPort`：物理端口能力

UART driver 或 driver-local frontend 唯一拥有 MMIO、IRQ、FIFO、boot-applied line configuration、raw RX handoff、line-error/overflow statistics 和最终 TX serialization。TTY 只持窄 capability，不访问寄存器或复制 raw state。

第一版 NS16550A 固定同时构造 output-only console backend 与 `TtyPort`；console 不消费 RX或提交 line configuration，`TtyPort` 的 pre-publish consumer binding 是唯一 RX 去向。普通 console/TTY TX 进入同一 IRQ-safe serialization；轮询实现必须按有界 batch 持锁，不能把任意用户 write 变成无界 IRQ-off 临界区。

#### `Terminal`：共享终端语义

同一 endpoint 的所有 open file 共享一个 `Terminal`。它唯一持有 committed termios、winsize、concrete N_TTY-like discipline、canonical pending/committed input、noncanonical input 与 readiness predicate。opened file 只保存 terminal reference 和确有语义的 per-open state；`O_NONBLOCK` 继续来自通用 open-file-description flags。

`Terminal` 可以使用粗粒度 guard，但 guard 不跨 user copy、sleep、TX wait、Event wake、Signal 或 task-topology call。最后一个 fd close 不销毁 Terminal，也不自动拆 controlling relation。

#### controlling-terminal relation：session 与 terminal 的单一绑定

TTY relation owner 中的一个 relation 唯一持有 terminal identity、stable session identity、foreground process-group identity 与 stale-detection generation。Session 与 Terminal lookup 可以指向同一 relation，不得分别缓存 binding 或 foreground PGID。

task topology 继续唯一拥有 Session/ProcessGroup membership。跨生命周期必须保存 stable identity并在 owner 边界重验；裸 SID/PGID 只用于 ABI lookup。`/dev/tty` open 按 current caller 的 session identity解析 live relation，后续 I/O 不使用 opener、最近 reader 或全局 PGID 猜测目标。

### 3. RX、line discipline 与 deferred effects

hard IRQ 把有界、有序的 byte/error records 发布到单一 raw handoff，累计 counter，并通知 deferred consumer；不运行 line discipline、生成 signal、唤醒 user waiter或递归 printk。预分配 ring、bounded fallible growth、per-port kthread 或其它 carrier 是 implementation choice，但 raw-nonempty predicate 才是 durable work truth，notification 只请求重验。

deferred consumer 按序 dequeue 有界 batch，在 terminal guard 内推进 discipline 并形成 echo、signal 与 readiness effects，释放 guard 后再执行 TX、wake 和 process-group signal generation。实现必须证明 handoff 与 terminal waiter 两层都没有 lost work/wake。

### 4. 输入队列、read 与 readiness

canonical mode 区分 pending edit 和 committed record：半行不可读，delimiter/`VEOF` 提交记录，`VERASE`/`VKILL` 不修改已提交输入，一次 read 不越过 record boundary。noncanonical mode 至少真实支持 `VMIN=1, VTIME=0`。

blocking read 与 poll/select 观察同一个 committed-input predicate并使用 publication + recheck。`O_NONBLOCK` 无数据时返回 would-block。TTY 可以在 guard 内把有界 prefix 暂存到 kernel buffer 并推进 queue，再由通用 VFS 路径 copyout；首版不为 post-validation user-copy fault 建立 rollback/replay transaction。

### 5. 输出、echo 与 console 共用端口

TTY write 接受任意 bytes，不继承 anonymous console 的 UTF-8 校验。`OPOST` 关闭时原样提交；启用变换时，partial progress 按已消费的用户输入 bytes 计量。echo 复用同一 output transform 与 port capability，但不在 terminal guard 内等待硬件。

TX 可以 polling、buffered 或 interrupt-driven；writable、drain、partial write 与 `TCSETSW` 必须来自实际 backend 能力。panic/early-console 可以有明确 best-effort 降级，但不是普通 TTY TX truth。

### 6. 第一版 termios 与 ioctl 下限

第一版至少真实支持：

- `TCGETS`、`TCSETS`、`TCSETSW`、`TCSETSF`；
- `ICANON`、`ISIG`、`ECHO`、`ECHOE`、`ECHOK`、`ECHONL`；
- `ICRNL`、`OPOST`、`ONLCR`；
- `VINTR`、`VQUIT`、`VERASE`、`VKILL`、`VEOF`、`VSUSP`；
- noncanonical `VMIN=1, VTIME=0`；
- `TIOCGWINSZ`、`TIOCSWINSZ`，初始 winsize 可固定为 `80x24`。

`TCSETSW` 在提交前满足真实 output drain，`TCSETSF` 额外 flush unread input。`TIOCSWINSZ` 只在尺寸变化后向 live foreground group 生成 `SIGWINCH`。初始 hardware-backed termios 从 boot-applied UART configuration 派生；需要 runtime line change 的请求稳定失败并保持旧 snapshot。精确 compatibility bit 与 errno 在 implementation gate 依据 executable oracle 冻结。

### 7. VFS、devfs 与 boot 接线

TTY 为每个 immutable port identity 确定稳定 `N`，发布 major 4、minor `64+N` 的 `/dev/ttyS<N>`。映射可以来自固定平台表、firmware alias 或确定性 allocator，但不得依赖并发 probe 完成顺序。

endpoint create 在 devfs publish 前完成 identity validation、Terminal/raw handoff、deferred consumer 和 open provider；publish 是单向可见 commit。TTY 另发布 major 5、minor 0 的 `/dev/tty`。console owner继续发布 major 5、minor 1 的 `/dev/console`，并唯一拥有 selected-console truth。

boot fd 0/1/2 安装选定的真实 Terminal file；显式 launcher/init 路径负责 `setsid()` + `TIOCSCTTY(arg=0)`，不能因“打开了 console”推断 controlling relation。第一版同时停止 NS16550A raw major 234 `CharDev` 的实现和注册。

### 8. Controlling terminal 与 job-control handoff

首版提供 `TIOCSCTTY(arg=0)`、session-leader `TIOCNOTTY`、`TIOCGPGRP`、`TIOCSPGRP`、`TIOCGSID`，以及 foreground `VINTR/VQUIT/VSUSP`、background read `SIGTTIN`、winsize `SIGWINCH` 与 detach/exit relation cleanup。

每次 read/write/ioctl 以同步 current caller 为准。TTY 在 relation owner 内取得 stable snapshot，guards-out 调用 topology/Signal 的窄 decision API 重验 caller、membership、mask/disposition 与 target；relation mutation随后返回 relation owner 重验 generation 才提交。完整 `Task`、topology guard 和 Signal state不得存入 `Terminal`。

`TIOCSPGRP` 的非 orphan 核心语义必须区分：

- foreground caller：验证后允许；
- background caller且 `SIGTTOU` blocked/ignored：允许 shell 收回 terminal；
- background caller且 `SIGTTOU` actionable：向 caller process group 生成 signal并返回 restart，mutation 不得提前提交。

TTY 只拥有 terminal-side policy 与 signal request；Signal、ProcessGroup 和 ThreadGroup jobctl继续提交 occurrence、stop/continue、user-entry barrier 与 parent report。

### 9. Lifecycle、relation cleanup 与延后的 hangup

session leader/controlling process exit与session-leader `TIOCNOTTY`终结 relation。TTY relation owner先撤销 `/dev/tty`、foreground check与后续 mutation 对旧 relation 的可发现性，再 guards-out完成 wake/drop。首版不生成 relation-disassociation `SIGHUP`/`SIGCONT`。

foreground group消失只使 selector失效，不拆 relation；newly orphaned stopped process-group判定及 effect继续属于 topology/jobctl。最后一个 fd close、relation detach和session exit都不删除已发布 endpoint。hardware hangup/backend fatal 的 fd/open/poll、relation effect与consumer quiesce在后续 target revision定义。

### 10. 并发与锁边界

- hard IRQ 不取得 sleepable terminal/topology/Signal lock，不做 user copy、policy wake、复杂 drop或递归日志。
- port guard不嵌套 terminal discipline；terminal guard不跨 TX wait、Event、Signal或topology call。
- relation与task topology不复制可变 state；跨域只传 snapshot/capability并在local owner重验。
- waiter和deferred consumer均使用 predicate + notification + recheck；wake不携带 durable truth。
- bounded queue overflow必须有明确drop policy与counter；重要容量进入 kconfig。
- cleanup先撤销可发现性，再执行 guards-out effect；publish前 rollback只处理未公开对象。

罕见 concurrent termios/RX/foreground race不必复现 Linux 的具体胜者，但结果必须等价于 owner-revalidated 的合法前态或后态，且不得产生 stale relation、session外signal、重复输入、lost wake、deadlock或内存安全问题。

## 接受边界

接受 R0 表示上述 owner、serial TTY ABI包络、两个 cutover unit与proof obligations可以进入实现；不表示任何 `TTY-*` contract已经生效，也不自动授权 Stage 0。data-plane checkpoint可以先执行 `TTY-DATA-CUTOVER`，但完整 closure仍要求 `TTY-JOBCTL-CUTOVER`和ash/vi验收。

以下属于 correctness boundary：单一 hardware/input/terminal/relation/jobctl owner，stable identity revalidation，guards-out cross-owner effect，bounded IRQ work，no lost wake/work，ABI不伪造与publish/cleanup原子性。以下属于 target guarantee：首版 `/dev/tty*`、termios/data-plane、controlling relation、`TIOCSPGRP`三分支与ash/vi包络。类型名、字段、锁类型、container、容量、deferred carrier、TX mode和模块拆分是 implementation preference。

改变 owner、ABI、`TTY-DATA-CUTOVER`/`TTY-JOBCTL-CUTOVER`组成、首版兼容包络、relation cleanup signal效果、console/TTY projection、endpoint lifetime或accepted limitation，必须回到 RFC review / `Target Renegotiation Gate`。PTY、hangup、runtime line configuration、`/dev/console` Terminal delegation及当前延期的job-control corner只能通过后续 revision或RFC扩展。

## 保留的工程余地

- `TtyPort`精确方法、error type、reference direction与pre-publish rollback shape。
- port identity到稳定 `N` 的固定表、firmware alias或确定性 allocator选择。
- raw handoff的owner-local container、容量、preallocation/noirq growth与deferred carrier。
- terminal guard闭包、input/effect containers与same-owner模块拆分。
- polling、bounded buffer、TX-empty IRQ或混合TX路线。
- `TCSETSW/TCSETSF`在所选TX路线下的精确setter/drain线性化。

这些选择由 implementation gate依照live source、内存预算和runtime evidence解析；若证据要求改变target或owner，必须停止而不是让probe接口自然沉淀。

## 备选方案

### 把 TTY 实现为通用 `CharDev`

拒绝。它会把 caller/session/readiness观察面扩散到所有字符设备，仍不能自然表达 shared Terminal与controlling relation。

### 用周期轮询保证 RX 进展

拒绝。轮询可以作为带删除条件的bring-up诊断，但不能成为raw-handoff正确性或lost-work补偿路径。

### 第一版建立可插拔 line discipline 或同时交付 PTY

延期。一个concrete discipline足以验证serial TTY；PTY还需要master/slave、ptmx/devpts、grant/unlock与独立hangup协议。

### 只交付 data plane

不作为RFC完成定义。data plane是有价值的checkpoint，但不能把interactive shell的job-control降级永久留给follow-up。

## 风险

- **跨 owner deadlock或stale identity：** 通过single-owner relation、stable identity、guards-out effect和local revalidation控制。
- **lost wake/work或错误readiness：** raw handoff与committed input各有唯一predicate，notification只触发重验。
- **IRQ storage与TX递归：** 有界capacity/counter、禁止RX IRQ printk、port-owned IRQ-safe serialization与bounded TX batch。
- **termios外形大于真实语义：** semantic flags真实执行，compatibility bits按明确policy处理，需要unsupported hardware behavior的update失败且不发布snapshot。
- **BusyBox定向fallback：** 自动oracle与交互验收覆盖真实relation、foreground switching、background policy和raw/canonical data path，禁止global PGID、recent-reader与success stub。
- **boot stdio被误当作controlling TTY：** 分别验证real Terminal fd与`setsid/TIOCSCTTY` acquisition。

## 收口

R0 已接受并建立 transaction，Stage 0 的 live interface、oracle、route 与模块边界审计已经关闭；
Stage 0 -> Stage 1 Resolution Gate已经完成，Stage 1为Active；Checkpoint 1/2已经关闭，Checkpoint 3按
driver-local quiescent probe / Late activation修正路线执行。全部`TTY-*`仍为Not Cut Over，
current contracts 与 register 未因本次入口或审计改变。已完成的设计 finding 保存在
[Tracking Issues](./tracking-issues.md)，本次执行证据与carrier owner处置见[事务日志](../../devlog/transactions/2026-07-23-tty-subsystem.md)，
历史调查保存在[背景材料](./backgrounds/index.md)。
