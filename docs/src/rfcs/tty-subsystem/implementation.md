# TTY Subsystem 迁移实施计划

**状态：** Closed / Stage 0-4 Closed
**最后更新：** 2026-07-24
**父 RFC：** [RFC-20260722-tty-subsystem](./index.md)
**目标与不变量：** [TTY Subsystem 目标与不变量](./invariants.md)
**当前契约：** [TTY data plane](../../contracts/tty/data-plane.md)中的五个ID与[TTY controlling relation / job control](../../contracts/tty/job-control.md)中的四个ID均为Active；Preserve项及其链接见[Contract Impact](./invariants.md#contract-impact)。
**当前修订：** R1
**事务日志：** [2026-07-23 - TTY Subsystem](../../devlog/transactions/2026-07-23-tty-subsystem.md)
**Contract Cutover：** `TTY-DATA-CUTOVER` Effective；`TTY-JOBCTL-CUTOVER` Effective

本文把 TTY target 解析为可滚动实施的阶段、probe、验证和 cutover 边界，不重新定义
[`index.md`](./index.md) 与 [`invariants.md`](./invariants.md) 已经拥有的 target、owner、ABI
或 proof obligations。R1现已关闭；Stage 0 获得单独授权后完成只读审计并关闭，
Stage 0 -> Stage 1 Resolution Gate 随后在新的明确授权下完成。Stage 1的三个checkpoint现已独立关闭；
Checkpoint 3采用获准的driver-local quiescent probe / Late activation路线完成production transport
candidate。Stage 1 -> Stage 2 Resolution Gate解析的四个checkpoint现已逐项完成，RV64自动matrix与用户人工
vi evidence达到floor，`TTY-DATA-CUTOVER`已经Effective并关闭Stage 2。Stage 3随后以单一vertical slice闭合
relation/caller/ioctl/cleanup并通过RV64验证。独立`3 -> 4` Implementation Resolution Gate依据live source
把最终Stage 4解析为Ready；用户随后明确授权Stage 4。首次自动matrix证明TTY侧
actionable `SIGTTIN`已经提交default stop且未消费input，但也暴露既有Signal/user-entry arbitration会在
jobctl park前丢失短命syscall restart capability；第12.3节记录获准的精确manifest扩展与修复后的自动floor。
第12.4节记录用户人工证据、register disposition、`TTY-JOBCTL-CUTOVER`与最终关闭；九个TTY contract ID现均Active。

## 1. 计划角色与 authority

发生冲突时按以下顺序判断：

1. `docs/src/contracts/` 描述已经生效的 shared rules；
2. `index.md` 与 `invariants.md` 描述本 RFC proposed / accepted-but-not-effective target；
3. 本文只拥有 stage 顺序、Ready stage、stage 内 checkpoint map、resolved manifest、probe、验证
   floor 与停止条件；
4. implementation transaction 只追加执行事实、review、验证、修正与 cutover 证据，不复制第二份计划；
5. register / current limitations 只描述实际开放缺陷和已接受限制，不替代 target 或阶段计划。

类型名、字段、锁、container、容量、deferred carrier、TX 模式、relation index 和窄 API 形状
只有在相应 Implementation Resolution Gate 冻结后才成为该 stage 的执行约束。它们不因本文
列出候选方案而升级为 target invariant。

## 2. 实施入口（不计入 Stage）

任何 stage 开始前必须先完成：

- 文档层 review 接受首个 target revision 为 R0，同步 RFC 状态、修订记录、Contract Impact 与仍有效的
  tracking-issue 结论；
- 建立 `docs/src/devlog/transactions/2026-07-23-tty-subsystem.md`，记录 R0、全部 prospective
  contract IDs、两个 cutover unit 和 Stage 0 的 Ready 链接；
- 建立 RFC 与 transaction 的双向链接，并同步 transaction index、当前双周 devlog 与
  `docs/src/SUMMARY.md`；current contract只允许增加获准的pending-successor导航，不提前改写effective rules；
- 对 R0 与 transaction 文档执行 `git diff --check`、`mdbook build docs` 与 link/anchor audit；
- 重新核对 live branch、HEAD、dirty state、register 与本计划；若 live owner 已漂移，先更新
  Stage 0 的 validation-only inventory，不沿用过期 source location；
- 用户明确授权 Stage 0。R0 acceptance、transaction creation 与 Stage 0 `Ready`
  都不自动构成这项授权。

如果 transaction 实际使用不同日期或 slug，必须在 Stage 0 激活前同步本文中的精确路径；
在路径仍未同步时，Stage 0 退回 `Outline`，不得把占位路径当作 frozen manifest。

## 3. 迁移原则

- `device::tty` 唯一拥有 terminal semantics、专属 FileOps、endpoint registry、controlling relation
  与 terminal-side access decision；UART、console、task topology、Signal 与 ThreadGroup jobctl
  保持正文规定的唯一 owner。
- 不扩大通用 `CharDev` 以承载 TTY caller/session/poll 语义。TTY 使用专属 open provider 和
  FileOps；UART 只暴露窄 transport capability。
- 第一版 NS16550A 固定同时提供 output-only console backend 与 `TtyPort`，并删除 raw `CharDev`
  实现/注册；TTY 是唯一 RX consumer，boot-applied line configuration 在运行期不可变，所有普通
  console/TTY TX 经过同一个 port-owned IRQ-safe serialization。不得为固定共存关系增加 claim、
  lease、mode state 或 bind/open/close personality switch。
- raw RX 的 storage 与 deferred carrier 必须由 live allocator、kthread/deferred facility、IRQ
  约束和 burst 证据选择。当前开放的 `ANE-20260622-IRQ-OFF-HEAP-ALLOCATION` 是强制审计输入；
  无法证明 bounded fallible growth 不进入 OOM/复杂 drop 时，优先解析预分配 bounded storage。
- 不为数据面提前构造 controlling relation，不为 job control 复制 termios/input/foreground/
  stop truth。跨 owner 操作使用 snapshot、stable identity、guards-out effect 与 owner-local
  revalidation/commit。
- endpoint publish 是单向 commit。任何公开 `/dev/ttyS<N>` 之前必须完成 raw handoff、consumer、
  Terminal、FileOps/open provider 与 rollback 边界；publish 后首版不增加 unpublish、编号复用或
  backend fatal teardown surface。
- data-plane checkpoint 与 `TTY-DATA-CUTOVER` 可以先于 job control；它们不能关闭 RFC，也不能把
  `job control turned off` 变成最终 limitation。`TTY-JOBCTL-CUTOVER` 只在 ash/vi 完整包络作为
  一个自洽单元验证后发生。
- 任何成功桩、全局 foreground PGID、最近 reader、opener identity、raw-mode 强制 signal、
  polling-as-progress-source 或 console/TTY 双 RX consumer 都是停止信号，不是迁移桥。
- 临时 bridge/probe 必须写明诊断、可见边界和删除/升级 gate；偶然跑通不让 generic workqueue、
  transport framework、manager object 或 public trait 自然沉淀。
- 实现反馈可以调整 future Outline、stage 拆分、内部 API、模块与验证安排；改变 target invariant、
  owner、shared contract、ABI、visible semantics、cutover unit 或 acceptance boundary 时，必须在
  cutover 前停止并进入 RFC review / `Target Renegotiation Gate`。
- 仓库 build 与 runtime 验证只走现有 wrapper。docs-only gate 不运行 QEMU/LTP；代码阶段的精确
  命令在相应 resolution gate 按 live build-system 入口冻结。

### 验证设施与证据分工

- 仓库新增一个轻量 `tty-test` app，作为可重复、可判定的主要 userspace oracle。它按 stage 逐步覆盖
  data plane、termios/readiness、controlling relation、foreground/background access 与 terminal signal，
  输出稳定的逐项 PASS/FAIL；不为了首版一次性预建完整 TTY 测试框架。
- 另建专用的轻量 TTY 验收 rootfs，不修改现有 pretest rootfs 的默认 `init -> user-test -> LTP`
  流程，也不让普通 `user-test` 默认阻塞在交互 shell。该 rootfs 只安装启动/回收所需的最小 launcher、
  `tty-test`、用户提供的 BusyBox 与实际运行所需的最小目录/挂载输入。
- BusyBox 是用户后续提供的只读外部 artifact，不提交到 Anemone 仓库，也不允许测试修改原件。接入时
  必须记录架构、链接/runtime 依赖、版本或其它可得 provenance、artifact identity，以及 ash、vi、stty
  等目标 applet 的实际可用性；具体本地 staging path 和 rootfs ingestion 入口由相应 resolution gate
  根据 live xtask/rootfs 接口冻结，不把个人 `etc/` 路径写成公共接口。
- 交互 launcher 必须建立可审计的 session/controlling-terminal/foreground 启动序列，再 exec
  BusyBox ash；仅从 `user-test` fork/exec 并继承可读写 fd 不足以证明 shell job control。launcher 只拥有
  测试编排、child reap 与退出/关机，不得携带 TTY success stub、foreground fallback 或内核专用后门。
- agent 负责构建、`tty-test`、可自动驱动的 QEMU/BusyBox smoke 与日志分析；用户只在 Stage 2 data-plane
  checkpoint 和 Stage 4 完整 job-control gate 执行冻结的人工 checklist，并回传 commit、架构、artifact
  identity、逐项结果、串口日志及日志无法表达的 echo/按键延迟/vi 显示现象。transaction 必须区分
  agent-run、user-run 与未运行证据，人工“看起来可用”不能替代自动 oracle 或 cutover proof。
- 人工交互发现的稳定语义缺陷应先尽可能固化为 `tty-test` 或可自动驱动的回归，再修改实现；确实只能
  人工观察的显示/交互现象保留精确操作序列与结果，不扩展为新的通用测试框架。

## 4. 阶段成熟度与滚动解析

- `Outline`：未来阶段，只固定目的、依赖、受保护边界和解析触发点；预计文件/目录不是写入授权。
- `Ready`：交付、审计、反馈假设、模块边界、scope envelope、可观测性、验证、退出条件、cutover
  和 `Resolved Write Set Manifest` 已完整解析，但没有自动获得执行授权。
- `Active`：已通过当前 RFC / transaction / 用户授权协议并开始执行。
- `Closed`：已按本阶段自己的 review、验证和退出条件独立关闭。

Stage N 必须先独立关闭，再执行单独的 `N -> N+1 Implementation Resolution Gate`。后者读取 live
source、实际 diff、review finding、验证证据、module pressure、target 与 current contracts，只把
下一个 Stage 展开为 `Ready`；不会回头把 Stage N 保持为 Active，也不会自动授权 Stage N+1。

`implementation.md` 是 Ready stage 与 frozen manifest 的唯一权威。transaction 只记录 preflight
证据、批准事实、生效点和本节链接。只有 Ready / Active stage 越过 frozen manifest 才是 write-set
扩展；future Outline 的收窄、扩大、拆分、合并或重排不算扩展。

### Stage 与 checkpoint 的两层边界

Stage 是 target 可达路径上的语义与 cutover 边界，不应同时被当成一次实现、一次 review 或一次提交
的最小单位。凡一个 Stage 横跨多个 natural owner-local slice、需要多种独立 oracle，或同时包含
foundation、wiring、publication、userspace acceptance 与 docs cutover，必须在该 Stage 的
Implementation Resolution Gate 中进一步解析为有序 checkpoint；不得等到代码已经铺开后才在
transaction 中临时切块。

- Stage 仍拥有前置依赖、受保护 target / contract、最终退出条件与 cutover；checkpoint 不建立第二套
  target、contract 或 cutover authority。
- Stage 从 `Outline` 进入 `Ready` 时，必须同时冻结完整 checkpoint map：每个 checkpoint 的目的、
  前置依赖、交付、允许写集或 stage manifest 子集、validation-only 输入、review / validation floor、
  停止条件、临时 bridge 退出点和 transaction write-back。整个 Stage 的
  `Resolved Write Set Manifest` 仍须闭合，不能只解析首个 checkpoint 后把 Stage 标成 `Ready`。
- 同一时刻至多一个 checkpoint 为 `Active`。每个 checkpoint 独立 activation、review、验证和关闭；
  checkpoint 关闭不自动激活下一个，也不等于 Stage 关闭或 contract cutover。若 Stage activation 已经
  预先授予连续执行权，transaction 仍须逐项记录每个 checkpoint 的激活与关闭事实。
- `Ckpt X -> Ckpt X+1` preflight 必须读取已完成 diff、review、验证、module pressure 与 bridge 状态，
  核对下一 checkpoint 仍能在 Ready stage 的 target、边界和 frozen manifest 内执行。只改变实现顺序、
  manifest 内分工或验证安排时更新本文并在 transaction 记录；需要扩大 manifest 或改变 owner、ABI、
  contract、cutover / acceptance boundary 时按正常扩展或 RFC review 停止。
- 只有最后一个 checkpoint 关闭且 Stage 自身的总体 exit / cutover 条件满足，Stage 才能关闭并进入
  `N -> N+1` gate。若 resolution gate 无法完整解析该 Stage 的 checkpoint map，说明当前 Stage 仍过粗；
  应先把 future Outline 拆成更小 Stage 或调整顺序，不能用“后续 checkpoint 再决定”制造部分 Ready。

## 5. 阶段路线图

| Stage | 成熟度 | 概括目的 | Contract Cutover | 解析触发点 |
| --- | --- | --- | --- | --- |
| Stage 0 | Closed | 只读闭合 live interface、oracle、carrier 候选与模块边界 | None | 已关闭；后续Stage 1 resolution亦已独立完成 |
| Stage 1 | Closed | 建立 unpublished port/Terminal transport vertical slice，闭合 IRQ、RX、TX 与 pre-publish transaction | None | 三个checkpoint已独立关闭；RV64 candidate验证完成，LA64未验证 |
| Stage 2 | Closed / Checkpoint 1-4 Closed | 交付 line discipline、termios、read/write/poll、`/dev/ttyS<N>` 与 real boot stdio | `TTY-DATA-CUTOVER` Effective | RV64自动与人工floor完成；Stage-wide audit关闭 |
| Stage 3 | Closed | 建立 controlling relation、`/dev/tty`、caller/topology handoff、foreground ioctls 与 cleanup | None | 单一vertical slice与RV64 floor已关闭；Gate P2 candidate成立 |
| Stage 4 | Closed | 闭合 terminal signals、background access、ash job control、register 与 current contract | `TTY-JOBCTL-CUTOVER` Effective | 自动、focused与用户人工RV64 evidence、review、register及contract cutover全部关闭 |

阶段顺序保护“先闭合真实数据通路，再接 relation/jobctl”的验证路径，不冻结最终文件数量。
当前粒度只表示语义路线，不表示每个 Stage 应在一个执行单元内完成。Stage 1的三个checkpoint和Stage 2的
四个checkpoint已经分别由对应resolution gate冻结；Stage 3以一个不拆checkpoint的vertical slice关闭。Stage 4同样
解析为一个不拆checkpoint的最终stage：terminal effect、background read、现有harness中的ash验收和原子cutover
共同证明同一组relation/job-control target，拆成更多阶段不会形成可独立cutover的能力。若自动或用户证据尚缺，
Stage 4保持Active / Awaiting Evidence，不额外建立验收stage或临时contract。

## 6. Stage 1 Ready：unpublished transport vertical slice

阶段成熟度：

- `Ready`。本节已冻结完整路线、checkpoint map、验证与 manifest；它不自动授权任何代码修改。
- Stage 1 只验证 Gate P1 candidate，不执行 `TTY-DATA-CUTOVER` 或其它 contract cutover。全部
  `TTY-*` 在本阶段关闭后仍是 Not Cut Over。

### 6.1 前置条件与受保护边界

- Stage 0 已独立关闭；本轮 resolution preflight 已读取其六类 evidence matrix、R0 target、register、
  current contracts、live source、`9fd95821` Stage 0 diff 与 clean worktree。
- 实现激活前重新记录 branch、HEAD、dirty state、active `kconfig` platform/features 和本节 manifest；
  用户对 resolution gate 的授权不构成 Stage 1、Checkpoint 1 或连续 checkpoint 的实现授权。
- 保护 `TTY-PORT-001`、`TTY-OUTPUT-001`、`TTY-ENDPOINT-001`、`TTY-LOCAL-003/004`。UART 唯一拥有
  MMIO、IRQ、FIFO、boot-applied line、raw queue 与最终 TX serialization；TTY 只持有窄 capability，
  console 只有 output projection，TTY 是唯一 RX consumer。
- 本阶段不得发布 `/dev/ttyS<N>` 或 `/dev/console`、替换 boot fd、修改 generic `CharDev` contract、
  建立 termios/line discipline/FileOps/controlling relation、生成 terminal signal、触碰 jobctl/topology/
  Signal/wait-core/scheduler，或实现 runtime line change、detach/hangup、PTY、generic workqueue/softirq。
- `ANE-20260622-IRQ-OFF-HEAP-ALLOCATION` 保持 Open。TTY 使用现有 `KThreadHandle::wake()`；不修改、
  包装或替代其 scheduler/wait-core 路径，不把该问题算作 Stage 1 failure 或 closure proof。

### 6.2 冻结路线与 capability 边界

1. **模块形状。** 先把当前单文件 `driver/serial/ns16550a.rs` 在同一 driver owner 内拆成
   `ns16550a/{mod.rs,regs.rs,port.rs}`。`mod.rs` 只保留 option parsing、probe/commit 与现有 KUnit；
   `regs.rs` 拥有 register constants/access、line programming 和 LoongArch early-console 继续使用的
   `Ns16550ARegisters`；`port.rs` 拥有 physical port、console/TTY projection、raw RX、TX 和 IRQ。
   `crate::driver::Ns16550ARegisters` 的现有可见路径和语义必须保持不变。
2. **窄 `TtyPort`。** 新建 `device::tty::{mod.rs,port.rs}`。`port.rs` 定义 crate-private
   `TtyPortId` 与 `TtyPort`；capability 只允许读取 immutable identity、检查 raw-RX predicate、按序
   dequeue 到 caller 提供的 bounded slice，以及提交 TX slice并返回实际接受 bytes。它不暴露 register、
   lock、raw container、`Task`、FileOps、termios、Signal或 lifecycle mutation。
3. **Unpublished attachment。** `device::tty::attach_unpublished_port()` 按 immutable `TtyPortId`
   拒绝重复 attach，建立尚不可由 devfs/VFS取得的 endpoint和每 port kthread。它返回
   `TtyPortAttachment` 与窄 `TtyRxNotifier`：attachment拥有pre-publish abort/stop/join责任；notifier只
   封装现有 `KThreadHandle::wake()`，不携带byte、count或request truth。Stage 1 worker用
   `rx_pending()`作为`KThreadCtx::wait_until()` predicate，反复从port dequeue固定栈上batch。
4. **Stage 1-only sink。** 本阶段尚无 Terminal/discipline，worker只对dequeue batch计数并丢弃；这条
   unpublished diagnostic sink是明确的Stage 1 bridge，不驱动readability或user-visible input。
   Stage 2必须在任何devfs publish前把它替换为同一endpoint的Terminal discipline提交路径；它不得越过
   `TTY-DATA-CUTOVER`，也不得扩展成generic callback/workqueue framework。
5. **Raw handoff。** NS16550A physical port在probe中用`Box::try_new`预分配
   `RingBuffer<u8, TTY_RAW_RX_CAPACITY_BYTES>`，以port-owned IRQ-safe lock保护单一FIFO。IRQ最多读取
   `NS16550A_IRQ_RX_BUDGET_BYTES`，保持顺序；满时drop-new并累计overflow，绝不覆盖旧byte。RX、drop、
   line-error、budget-exhaustion和notification计数只用于诊断，不参与predicate或状态转换。
6. **Notification publication。** IRQ在raw lock内先发布byte/counter，并只记录是否发生
   empty-to-nonempty；释放raw lock后才调用`TtyRxNotifier::wake()`。worker持续drain到predicate为false，
   然后用现有Event的register-plus-recheck wait。wake只促使重验，ring nonempty始终是durable work truth。
7. **Fixed projections与raw删除。** 同一`Arc<Ns16550APort>`固定构造output-only
   `Ns16550AConsole`和`Ns16550ATtyPort`；没有claim、lease、mode或open/close switch。删除NS16550A
   `CharDev` impl、`register_char_device()`、dynamic minor/bookkeeper，以及仅剩该consumer的
   `devnum::char::major::RAW_SERIAL`和对应devnum KUnit数组项；generic CharDev代码保持不变。
8. **Bounded TX。** console与TTY projection都调用physical port的同一TX入口。该入口用独立
   IRQ-safe TX lock，最多在一个临界区处理`NS16550A_TX_BATCH_BYTES`，每个byte最多轮询
   `NS16550A_TX_POLL_ITERATIONS`次；timeout释放lock并返回partial progress，console另累计未提交byte，
   不递归printk。任意长度caller buffer因此被拆成有界临界区；RX IRQ不取得TX lock且不打印。
   本checkpoint只证明port-owned TX serialization的临界区受这两个参数约束；generic
   `console::output()`在backend call外层持有console registry IRQ-save guard是既有console-owner边界，
   不得把本阶段证据扩大为任意console record的端到端IRQ-off latency proof。若后续验收要求后者，必须
   另行扩展console owner/write set，而不是在NS16550A内伪造证明。
9. **Immutable identity与line truth。** `TtyPortId`从platform device对应OF node的canonical full path
   构造并拷入固定容量identity；路径超长或不是OF node时在attach前失败，不回退到probe顺序、dynamic
   minor或局部KObject basename。validated stdout options与divisor保存在physical port的immutable
   applied-line snapshot；runtime不从register反推，也不提供apply/rollback API。
10. **Driver-local two-window activation。** live physical discovery早于`kthreadd`，因此early synchronous
    probe只完成firmware/identity/line解析、MMIO mapping、port/ring/IRQ-private所需对象分配，强制关闭UART
    RX interrupt，安装driver-local `Quiescent` port并注册output-only console；它不调用`request_irq()`、
    不启用RX、不进入TTY unpublished registry。platform bus的driver binding不是TTY endpoint/capability
    publication，`Quiescent`也不得被IRQ或TTY当作active truth。`Late` initcall在`kthreadd`与全部CPU local
    init完成后先通过driver device list取得锁外snapshot：第一次`for_each_device()`只计数，锁外fallible
    reserve精确容量，第二次在同一IRQ-save read guard内只clone device `Arc`并push到已经预留的Vec，且push前
    assert容量未漂移；释放driver guard后才逐个activation。该two-pass snapshot依赖boot期不再并发注册本driver
    device的source proof，不扩大`DriverBase` API。每个port依次执行TTY duplicate validation、kthread spawn、
    notifier/consumer binding和IRQ private `Arc`准备，最后调用`request_irq()`。attach或request失败必须
    abort attachment、撤unpublished registry并同步stop/join worker；只允许保留有界boot-lifetime MMIO、
    port、driver binding与TX-only console，device state的attachment slot继续为`None`且RX保持关闭。
    `Ns16550ADevice`中的`SpinLock<Option<TtyPortAttachment>>`是唯一activation truth：`None`为Quiescent，
    `Some`为Active，不另设phase字段；attachment不得存入physical port，否则会形成
    `port -> attachment -> endpoint -> port`强引用环。request成功是不可逆commit，其后只执行不可失败的
    attachment slot `None -> Some`提交和RX enable；先存slot再启RX，IRQ handler不读取该slot。IRQ永远不观察optional notifier，
    因为notifier绑定前不存在已注册IRQ。该two-window生命周期是未来generic deferred-probe能力到来前的
    driver-local bridge；当bus能在`kthreadd` dependency未满足时defer并重试probe，必须删除Late initcall和
    `Quiescent -> Active`迁移桥，把同一activation transaction收回单次成功probe。Late activation失败是
    可诊断的启动失败状态，不计作Stage 1 closure成功。

### 6.3 Checkpoint map

#### Checkpoint 1 — NS16550A same-owner split-only

**目的与交付：** 只完成`ns16550a.rs`到`ns16550a/{mod.rs,regs.rs,port.rs}`的物理移动；保留当前raw
`CharDev`、minor/bookkeeper、probe/IRQ/console行为、option parser、所有KUnit和public re-export。不得顺手
引入TTY、改锁、改日志、改register顺序或修复既有rollback TODO。

**允许写集：** Stage manifest中的旧`ns16550a.rs`删除和三个新driver-local文件；transaction只追加
activation、diff/review/validation与closure。其它stage文件仍不可写。

**Review / validation floor：** `git diff --check`与`just fmt kernel --check`；记录入口platform后依次执行
`just conf switch qemu-virt-rv64-pretest`、`just build`、`just conf switch qemu-virt-la64-pretest`、
`just build`，最后切回入口platform并再次`just build`。确认两种架构均编译KUnit且LoongArch bootstrap仍
通过原re-export构造registers；用diff/source audit证明函数体、visibility、KUnit assertions和probe调用
顺序只发生必要的路径/`use`调整。

**停止条件：** 若保持early-console路径需要改变public trait、register API、owner或行为，停止并上报；
不得把该变化伪装为split-only。Checkpoint 1关闭不自动激活Checkpoint 2。

#### Checkpoint 2 — Dormant TTY port/attachment core

**目的与交付：** 新建`device::tty` module、crate-private `TtyPortId`/`TtyPort`、unpublished registry、
`TtyPortAttachment`/`TtyRxNotifier`和Stage 1-only drain sink；用fake port KUnit闭合duplicate identity、
notification-before-wait、notification-after-wait、concurrent drain、predicate-stays-true、FIFO batch与
pre-publish abort/worker exit。production NS16550A仍走Checkpoint 1保留的raw路径，本checkpoint不改变
boot行为。

**允许写集：** `device/mod.rs`与计划新建的`device/tty/{mod.rs,port.rs}`，以及transaction。KUnit留在
对应owner文件的`kunits` module，不新建通用test framework或`tests.rs`。

**Review / validation floor：** `git diff --check`、`just fmt kernel --check`与`just build`；运行
`./scripts/run-user-test-rv64.sh etc/preliminary/images/sdcard-rv.img build/tty-stage1-c2-rv64.log`，在新增
fake-port/attachment cases全部为`ok`且全量KUnit打印`All tests passed!`后用QEMU monitor正常退出；已经
开始的post-KUnit LTP不作为证据。source/lock review确认registry guard不进入worker wait/port drain，
stop/join不持registry或port guard，diagnostic字段不驱动行为。

**Bridge / stop：** Stage 1-only sink保留到Stage 2 consumer replacement gate；除此之外不得新增
callback、polling watchdog或generic worker。如果attachment必须观察Task/scheduler内部、扩大
`KThreadHandle`或用周期poll保证进展，停止Stage并回写route evidence。Checkpoint 2关闭不自动激活
Checkpoint 3。

#### Checkpoint 3 — NS16550A production transport wiring

**目的与交付：** 按6.2修正后的driver-local two-window路线把NS16550A接入unpublished attachment；early
probe只建立quiescent port与output-only console，Late finalize完成attachment、IRQ commit和RX enable；加入四个non-zero kconfig参数
`tty_raw_rx_capacity_bytes = 4096`、`ns16550a_irq_rx_budget_bytes = 256`、
`ns16550a_tx_batch_bytes = 16`、`ns16550a_tx_poll_iterations = 65536`及parser/default/generation tests；
完成fixed console + TTY projections、raw 234删除、immutable identity/applied-line snapshot、RX FIFO/
counter/empty-to-nonempty notification、有界TX和request-IRQ commit/rollback。IRQ路径不保留普通日志；
worker只在首次成功drain输出一条明确标为Stage 1 diagnostic的process-context摘要，Stage 2 consumer接线时删除。

**允许写集：** 本checkpoint取得Stage manifest内除Checkpoint 1/2已关闭文件之外的`device/devnum.rs`、
`conf/.defconfig`和`scripts/xtask/src/config/kconfig.rs`；可继续修改`ns16550a/{mod.rs,regs.rs,port.rs}`、
`device/tty/{mod.rs,port.rs}`与`device/mod.rs`。`kconfig`、`anemone-kernel/src/kconfig_defs.rs`和
`platform_defs.rs`只能由repository config/build入口生成，是validation output，不得手改或提交为source。

**KUnit / source proof：** 覆盖kconfig zero rejection/default generation、ring FIFO/drop-new、只在首次
empty-to-nonempty通知、budget边界与剩余work、line-error计数、TX batch/poll timeout/partial progress、
duplicate attach和request前abort。source audit必须证明early probe只发布`Quiescent` port/console且RX关闭，
Late activation不依赖其它Late consumer的顺序；driver device snapshot只在IRQ-save guard内计数/clone到
预留Vec，spawn/attach/request/join全部在guard外；device-state attachment slot是唯一activation truth且port
不反向拥有attachment；IRQ private data直接持有预分配port `Arc`，不再
查dynamic bookkeeper；consumer/ring在RX enable前可达；所有wake在raw guard外；IRQ无format/log、Vec/
VecDeque growth、complex drop、sleepable lock、Event/Signal/topology调用；TX lock内工作受两个参数约束；
attach/request失败没有registry/worker/IRQ/RX残留，request成功后没有fallible步骤。

**Runtime floor：** 运行`./scripts/run-user-test-rv64.sh etc/preliminary/images/sdcard-rv.img
build/tty-stage1-rv64.log`；在PTY中等待NS16550A attach完成后注入超过一个RX budget、包含可识别顺序的
ASCII burst，再用QEMU monitor退出。日志必须包含全量enabled KUnit的`All tests passed!`、一次且仅一次
Stage 1 process-context drain摘要、无IRQ递归printk/死锁/panic；post-KUnit LTP若已开始不属于本阶段证据。
本轮依用户明确validation disposition不运行LA64 build/runtime；RV64证据不得外推为LA64 compile、runtime
或hardware RX proof。

**停止条件：** early probe注册IRQ、启用RX或进入TTY registry；Late activation前存在IRQ可观察的optional
notifier；request-IRQ后仍存在可返回失败步骤却无unwind；raw endpoint/major/bookkeeper仍可达；
console消费RX；TTY访问register；overflow覆盖旧byte或静默；worker只能靠poll前进；wake发生在raw guard内；
TX可能在一个临界区处理任意长度buffer；IRQ新增allocation/log/complex drop；或必须修改console、IRQ core、
kthread/wait/scheduler/shared contract才能继续。以上任一命中即停止，不用局部bridge绕过。

### 6.4 Stage-wide审计、可观测性与验证

- 审计全树`RAW_SERIAL`、NS16550A `CharDev`/`register_char_device`/minor bookkeeper、RX byte discard、IRQ
  printk、console RX、TX register bypass、claim/lease/mode、TTY polling timer/watchdog和devfs `/dev/tty*`
  publication；除历史docs外，前五类production旁路必须为零，devfs publication必须仍不存在。
- 审计所有新增allocation：ring/Arc/opaque/worker/registry只允许在probe/attach process context；TTY-owned
  IRQ/storage/counter/notifier调用前后不得新增allocation、OOM side effect、format、普通日志或复杂drop。
- per-port production counters至少包括RX accepted、drop-new overflow、line error、IRQ budget exhaustion、
  empty-to-nonempty notification、TX accepted/timeout和console unsubmitted bytes。它们使用relaxed
  diagnostic atomics或等价snapshot，字段旁明确“不参与predicate、ordering或state transition”。
- correctness assertion至少覆盖duplicate attach不可提交、notification只对应已发布nonempty transition、
  worker dequeue后不保留replay copy、abort先撤registry再stop/join，以及request IRQ前consumer/notifier已绑定。
  不在cleanup中先panic再撤销资源。
- 每个checkpoint执行`git diff --check`、`just fmt kernel --check`和其定向floor；Stage最终另执行
  `just build`、上述RV64 wrapper/KUnit/RX burst、`mdbook build docs`和manifest/source
  audit。不得把build、KUnit、RV64 QEMU、LA64 build或source audit互相升级为未运行层级的证明。

### 6.5 Stage contract、退出与授权

Contract cutover：

- None。Stage 1 candidate不更新`docs/src/contracts/`、register或任何`TTY-*` effective状态；
  `TTY-DATA-CUTOVER`只属于Stage 2。

Stage-wide停止条件：

- 任一checkpoint需要扩大下列resolved manifest，先记录扩展理由、拟新增owner/files、contract与验证影响，
  获得批准并更新本文/transaction后再继续。
- owner、public API、ABI、visible semantics、cutover unit、accepted limitation或acceptance boundary变化，
  进入RFC review / Target Renegotiation Gate；不得以Stage 1路线修正自行接受。
- 不因现有`KThreadHandle::wake()`底层scheduler风险停止TTY；只有TTY自身违反本节IRQ/storage/notification
  边界，或实现试图修改/替代wait-core/scheduler，才命中Stage 1停止合同。

退出条件：

- 三个checkpoint都已独立review、验证、关闭，所有临时状态与bridge disposition写入transaction；
- production NS16550A只保留fixed console + `TtyPort`，raw major 234路径为零，RX/TX/identity/applied-line
  owner与6.2一致；request失败没有unpublished registry/worker残留，request成功后无fallible half-commit；
- focused KUnit、RV64 runtime burst、source/lock/manifest audit达到本节floor，且结论按证据层级记录；LA64
  build/runtime按本轮用户处置不执行且明确记录为未验证；
- transaction把Stage 1记为Closed并明确Stage 2 resolution输入；Stage 2是否Ready不是Stage 1关闭条件。

Resolved Write Set Manifest：

允许tracked source写入：

- 删除`anemone-kernel/src/driver/serial/ns16550a.rs`；新建
  `anemone-kernel/src/driver/serial/ns16550a/{mod.rs,regs.rs,port.rs}`；
- `anemone-kernel/src/device/mod.rs`；新建`anemone-kernel/src/device/tty/{mod.rs,port.rs}`；
- `anemone-kernel/src/device/devnum.rs`，仅删除raw serial major及更新同页devnum KUnit；
- `conf/.defconfig`与`scripts/xtask/src/config/kconfig.rs`，仅加入上述四项参数、non-zero validation、
  generated definitions和定向parser/default tests；
- 本文只用于获准的Ready-manifest route correction/expansion；
- `docs/src/devlog/transactions/2026-07-23-tty-subsystem.md`只追加checkpoint activation、diff、review、
  validation、stop/correction、closure与Stage 1 handoff事实。
- `docs/src/rfcs/tty-subsystem/index.md`、`docs/src/rfcs.md`、当前双周devlog与transaction index只同步
  Stage/checkpoint lifecycle导航，不改变R0 target、tracking issue或contract文本。

Validation-only inputs：

- `anemone-kernel/src/{utils/ring_buffer.rs,task/kthread/,sched/event.rs,exception/intr/irq.rs,sync/spinlock.rs}`；
- `anemone-kernel/src/{driver/mod.rs,driver/serial/mod.rs,arch/loongarch64/bootstrap.rs}`；
- `anemone-kernel/src/device/{console.rs,char/,discovery/,bus/platform/}`、`anemone-kernel/src/fs/devfs/`、
  `anemone-kernel/src/main.rs`；
- `Justfile`、`scripts/xtask/src/tasks/{build/,conf.rs,qemu.rs}`、RV64/LA64 wrappers、active `kconfig`、
  platform/rootfs configs、generated defs与build/QEMU logs；
- 本RFC、transaction、current contracts、register和Stage 0 commit/diff。

不允许触碰：

- `device/console.rs`、generic `device/char/`、IRQ core、ring-buffer utility、kthread/Event/wait/scheduler、
  VFS/devfs/main、task topology/Signal/jobctl、apps/rootfs manifests/test profiles、current contracts/register；
- alpha worktree、共享`etc/{preliminary,final,xref}`资源、测试盘master和用户BusyBox artifact。

实现责任：

- implementer一次只激活一个checkpoint，保持diff在该checkpoint子集内并追加transaction事实；reviewer按
  C1行为保持、C2 owner/lifecycle/no-lost-work、C3 IRQ/TX/rollback/raw-removal分别审查；integrator在Stage
  closure前执行全manifest、旁路、双架构和证据分层审计。任何角色都不能把Ready当作执行授权。

## 7. Stage 2 Ready：terminal data plane 与 `TTY-DATA-CUTOVER`

阶段成熟度：

- `Closed / Checkpoint 1-4 Closed`。四个checkpoint均按各自activation、review、validation和closure边界完成；
  R1 Target Renegotiation Gate、C4 activation preflight、RV64自动matrix、用户人工vi checklist与stage-wide
  audit均已关闭，`TTY-DATA-CUTOVER`为Effective。
- Stage 2只交付relationless serial Terminal data plane。它可以执行`TTY-DATA-CUTOVER`，但不建立
  controlling relation、`/dev/tty`、foreground selector或job-control signal effect，也不激活
  `TTY-REL-*`、`TTY-JOBCTL-*`、`TTY-LIFE-*`或完整`TTY-ABI-001`。

### 7.1 前置条件、证据输入与受保护边界

- Stage 1已经独立关闭。live production candidate为fixed output console + unpublished `TtyPort`；
  NS16550A raw major 234路径为零，raw RX fixed ring、per-port kthread、bounded TX serialization、immutable
  OF-path identity和pre-publish abort已经接线。RV64 build/KUnit/QEMU/371-byte burst有证据；LA64 build/runtime、
  hardware RX和Stage 2 ABI仍为Not Run，不得从RV64结果外推。
- 实现激活前重新记录branch、HEAD、dirty state、active platform、本节manifest以及BusyBox staging状态。
  R1 acceptance输入使用用户指定的初赛RV64 musl BusyBox 1.33.1：RISC-V static ELF，SHA-256
  `fd9cb9dc66ba740dc94b055b564de0597453adfceef9be158b3774ca58b95241`；已通过可执行`--list`核对
  `ash`、`stty`、`vi`、`mount`、`stat`与`poweroff`。artifact和测试盘master始终只读，不提交仓库，也不在
  公共接口中记录个人挂载路径。LA64 artifact/build/runtime在R1中明确为Not Run，不进入C4 staging或cutover proof。
- 用户把`anemone-apps/`、`anemone-rs/`和`anemone-abi/`授权为本RFC剩余阶段的长期scope envelope。
  该授权避免后续stage因自然userspace/ABI工作重复申请，但不替代每个Ready stage的精确manifest；本阶段
  只允许7.7列出的文件和`anemone-apps/tty-test/**`，不得借scope envelope清理无关代码。
- 保护`TTY-PORT-001`、`TTY-TERM-001`、`TTY-INPUT-001`、`TTY-OUTPUT-001`、`TTY-ENDPOINT-001`与
  `TTY-LOCAL-001..005`。UART仍唯一拥有register、raw queue、boot-applied line和最终TX serialization；
  Terminal唯一拥有committed termios/winsize、discipline/input/output queue与readiness predicate。
- 本阶段不得修改generic `CharDev`、iomux/wait-core/scheduler、task topology、Session/ProcessGroup/Signal/
  jobctl，或增加PTY、hangup、runtime line reconfiguration、`/dev/tty`、foreground PGID、relation lookup、
  last-close teardown、polling watchdog、console RX或Terminal-backed`/dev/console`。

### 7.2 Terminal、discipline与worker路线

1. **Same-owner module shape。** 保留`device::tty::{mod.rs,port.rs}`作为registry/worker与transport
   boundary；新增`terminal.rs`、`discipline.rs`、`file.rs`和`endpoint.rs`。`Terminal`不叫manager，也不
   缓存port、worker handle或endpoint identity；endpoint组合port、Terminal和open provider，worker拥有
   两者的执行路线。若实际代码显示该五文件形状反而制造循环依赖，可在同一owner内合并文件，但不得新增
   public abstraction或越过7.7 manifest。
2. **唯一terminal truth。** 每个unpublished endpoint创建一个shared `Terminal`。它的单一guard内保存
   owner-local semantic termios snapshot、winsize、canonical pending edit、committed input records、
   noncanonical committed bytes、bounded output queue以及read/write/drain waiter registrations；raw
   asm-generic bit layout只在Checkpoint 2的ioctl边界转换，不成为第二份状态。opened file只持Terminal
   reference和不形成反向环的窄endpoint notifier；termios、input、winsize和`O_NONBLOCK`不做per-open缓存。
3. **容量与预分配。** 新增四个non-zero kconfig项：`tty_canonical_line_capacity_bytes = 4096`、
   `tty_input_capacity_bytes = 4096`、`tty_output_capacity_bytes = 4096`和
   `tty_worker_batch_bytes = 256`。Terminal/endpoint在publish前完成相应fallible allocation和精确reserve；
   guard内不得触发隐式增长。capacity exhaustion必须形成稳定backpressure或明确diagnostic counter，不能
   覆盖旧input、丢失已接受write byte或依赖OOM side effect。
4. **Concrete discipline。** input先应用`ICRNL`，再按committed snapshot处理canonical/noncanonical、
   editing、signal controls和echo；不建立可插拔line-discipline trait。canonical pending在newline时提交
   包含delimiter的record；`VEOF`不作为byte入队，而是提交EOF boundary：空pending使下一次read返回一次0，
   非空pending使该record立即可读且在record耗尽时消费boundary，不额外产生第二次0。`VERASE`只删除pending
   最后一个byte，`VKILL`只清pending；均不得修改已提交record。noncanonical只接受`VMIN=1,VTIME=0`，任意
   committed byte即可读。
5. **Relationless `ISIG`。** Stage 2真实识别`VINTR/VQUIT/VSUSP`：control byte不进入input，默认
   `NOFLSH`未启用时flush pending/committed input和尚未提交hardware的Terminal output，并按echo policy
   形成可见effect。由于本阶段不存在controlling relation/foreground target，signal request的合法结果是
   `NoForegroundRelation`并只累计限频诊断，不调用Signal。Stage 4在relation存在时替换这条no-target effect；
   不允许把control byte当普通input、静默success-stub或提前加入全局PGID fallback。
6. **同一output truth。** Terminal拥有bounded backend-byte queue；ordinary write先按当前snapshot对一个
   用户byte完整执行`OPOST/ONLCR`变换，只有整个变换token都成功入队才把该用户byte计为consumed。
   `ONLCR`的`\n -> \r\n`因此不会产生半个用户byte进度。echo复用同一transform与queue；write、echo和
   drain不直接绕过queue调用port。queue capacity能够接纳当前模式下任意一个最大token时才是writable。
7. **一个endpoint worker。** Stage 1 per-port kthread升级为endpoint worker，同时以
   `port.rx_pending() || terminal.output_pending() || drain_check_pending`和尚未处理完的worker-local RX batch
   作为durable work predicate。worker从port transfer batch后，该batch remainder是唯一pending raw owner；
   不复制replay shadow。它按有界轮次推进discipline、提交echo/output、调用port bounded `submit_tx()`并
   重验predicate，仍有工作时yield；只在所有predicate为false时register-plus-recheck sleep。
8. **Notifier与引用关系。** driver attachment是worker handle的唯一长期strong owner；IRQ context持其
   RX notifier projection，published open provider只持attachment-owned wake source的`Weak`引用，成功open
   后的file可在自己的fd生命周期内持窄strong wake capability。`Terminal`、port、endpoint和长期provider
   都不得持`KThreadHandle`，防止`endpoint -> handle -> worker arg -> endpoint`强引用环。read释放input
   capacity、write入队output、termios发起drain以及IRQ empty-to-nonempty都只请求worker重验；wake count
   不是状态真相。publish前必须证明weak source在endpoint boot lifetime内由driver attachment稳定拥有。
9. **Guard边界。** Terminal guard不跨port dequeue/TX/idle query、user copy、Latch trigger/drop、worker
   sleep/yield或复杂drop。worker先在guard内commit state并detach wake effects，再释放guard执行port/wake；
   FileOps用bounded kernel buffer与generic VFS copy contract，post-validation copy fault不回滚或replay。

### 7.3 Port、termios与ioctl policy

1. **窄port扩展。** `TtyPort`增加immutable line snapshot与nonblocking TX-idle query；NS16550A从Stage 1
   `AppliedLine`映射公开的owner-neutral baud/parity/data-bits snapshot，idle只观察真实transmitter-empty
   hardware状态。TTY不读取register或修改line。endpoint worker拥有TX progress；`TCSETSW/F` waiter通过
   register-plus-recheck等待Terminal queue为空且port真实idle，不用周期timer或caller busy-poll。
2. **asm-generic ABI。** `anemone-abi`新增独立`tty::linux`模块，以Linux 6.6.32 asm-generic值定义
   `tcflag_t`/`cc_t`形状、`NCCS=19`、`Termios`、`Winsize`、目标flags/control-char index和
   `TCGETS/TCSETS/TCSETSW/TCSETSF/TIOCGWINSZ/TIOCSWINSZ`；kernel和`anemone-rs`共同消费它，禁止从host
   libc header或kernel-private layout复制第二份数值。结构layout/size/alignment和ioctl值用compile-time
   assertion与ABI tests固定。
3. **初始snapshot。** `c_line`固定为0；hardware baud、character size和parity从port boot-applied snapshot
   映射，另固定`CREAD|CLOCAL`，runtime均不可改变。初始semantic policy为`ICRNL`、`OPOST|ONLCR`、
   `ICANON|ISIG|ECHO|ECHOE|ECHOK`，winsize为80x24；control table固定为`VINTR=^C`、`VQUIT=0x1c`、
   `VERASE=DEL`、`VKILL=^U`、`VEOF=^D`、`VSTART=^Q`、`VSTOP=^S`、`VSUSP=^Z`、`VREPRINT=^R`、
   `VDISCARD=^O`、`VWERASE=^W`、`VLNEXT=^V`、`VMIN=1`，`VTIME/VSWTC/VEOL/VEOL2`及未命名slot为0。
4. **setter validation。** `TCSETS*`先对完整candidate做validation，再原子提交snapshot。仅允许改变
   `ICANON/ISIG/ECHO/ECHOE/ECHOK/ECHONL`、`ICRNL`、`OPOST/ONLCR`、列出的control chars以及
   `VMIN/VTIME`；BusyBox vi清除的`IXON`、`IEXTEN`等compatibility bit在旧snapshot本来为0时可保持0，
   但不能被设置。未支持mask、其它`c_cc`、`c_line`或hardware line bits只允许与旧snapshot相同；任何
   实际改变、noncanonical非`VMIN=1,VTIME=0`或未知组合返回`EINVAL`并完整保留旧snapshot。不得静默接受
   会产生未实现可见行为的变化。asm-generic `_POSIX_VDISABLE`值0可用于列出的特殊字符；discipline
   必须把对应动作视为disabled，不能把普通NUL input误判为control action。
5. **setter ordering。** `TCSETS`通过validation后立即commit；`TCSETSW`先等待真实output drain，再在
   terminal guard内重验generation和candidate后commit；`TCSETSF`执行同一drain/revalidation，再原子flush
   unread canonical/noncanonical input并commit。被signal中断、copy fault或revalidation失败时不发布部分
   snapshot/flush；使用现有VFS/wait restart contract，不新造TTY restart state。
6. **winsize。** `TIOCGWINSZ`读取shared snapshot；`TIOCSWINSZ`原子提交新值，值不变为成功no-op。
   Stage 2没有foreground relation，因此change不会生成`SIGWINCH`，并以no-target counter保持可观测；
   Stage 4加入relation effect。unknown ioctl稳定返回`ENOTTY`；userspace pointer fault由`IoctlCtx`/uaccess
   返回`EFAULT`，不能改成`EINVAL`。
7. **Userspace wrapper。** `anemone-rs`只增加raw `ioctl`/`ppoll`所需syscall入口和typed tty helper，供
   `tty-test`共享同一ABI；不建立libc-sized通用termios层、不预建Stage 3 controlling-terminal wrapper。

### 7.4 FileOps、readiness、identity与publication

1. **TTY-owned files。** `file.rs`提供byte-stream read/write/ioctl/poll、`ESPIPE` seek/positioned I/O和
   status-flag validation。read count为0立即返回0；blocking read/writes按同一predicate睡眠，
   `O_NONBLOCK`每次从`FileIoCtx`读取，无进度时返回`EAGAIN`，已有进度时返回short count。write接受任意
   bytes，不执行anonymous console的UTF-8校验。
2. **Read boundary。** canonical read只从一个committed record取prefix，允许同一record被多次short read，
   不越过下一个record；empty VEOF record返回一次0并被消费。noncanonical返回当时可用prefix。
   readable唯一等价于“存在可消费committed record/byte”；raw ring、worker wake、canonical pending和
   empty output queue都不是readable truth。
3. **Poll/select。** `READABLE`使用上述input predicate，`WRITABLE`使用当前output transform所需最大token
   capacity；不宣称HUP/ERR。snapshot request直接返回ready/unsupported；register request在Terminal guard内
   check、保存`LatchTrigger`、recheck后才返回`Armed`。read/write/flush/drain在同一guard内detach对应trigger，
   guard外trigger，覆盖notification-before/after-register与stale-trigger cleanup。
4. **确定性编号。** 在所有boot-time attachments仍unpublished且Late initcall已经全部完成后，以
   `TtyPortId` canonical OF full path对全体endpoint排序并分配连续`N`；名称、identity与devnum作为同一次
   mapping commit形成`ttyS<N>`和major 4/minor `64+N`。不使用probe/activation完成顺序、dynamic minor、
   KObject basename或chosen registration顺序；duplicate/overflow在任何publish前失败。
5. **Console selection owner。** console registration可携带可选opaque terminal identity；
   `console::on_system_boot()`在启用/淘汰early console的同一owner窗口提交唯一selected-terminal identity。
   TTY只读该identity并在自己的sorted registry重验；没有selected TTY identity时使用最低确定性identity。
   TTY不得保存第二份console selection truth或让console查询TTY内部guard。
6. **显式boot finalize。** 所有Late initcall返回后、mount rootfs/exec init前，boot coordinator先调用
   console-owned`/dev/console` publish，再调用TTY endpoint prepare/publish和boot-terminal selection。
   prepare阶段完成Terminal、worker、open provider、所有编号/node descriptor和boot file创建；任一失败
   必须在零devfs visibility下停止启动。随后按确定顺序执行`devfs::publish`单向commit；首个node可见后
   任一后续publish失败都视为boot-fatal invariant violation，不尝试runtime unpublish/renumber。
7. **Console与stdio。** console owner发布major 5/minor 1 `/dev/console`，继续使用现有read-EOF、
   output/UTF-8和non-TTY ioctl语义；它不委托到Terminal，也不成为boot stdio。`exec_init_proc()`以选定
   Terminal的三个正常File分别安装fd 0 Read、fd 1/2 Write；termios/winsize/input在三者及后续
   `/dev/ttyS<N>` opens之间共享。无法选出Terminal时fail closed，不回退anonymous stdin EOF。

### 7.5 Checkpoint map

#### Checkpoint 1 — Terminal discipline 与双向worker core

**目的与交付：** 新建Terminal/discipline state，加入四项kconfig容量、port line/idle capability、bounded
output queue和双向endpoint worker；删除Stage 1 discard/first-drain diagnostic sink，以fake port KUnit闭合
canonical/noncanonical、VEOF/edit/echo/transform/backpressure、RX batch transfer、TX partial retry、drain和
notifier引用关系。endpoint仍unpublished，不增加FileOps/UAPI或改变boot stdio。

**允许写集：** `device/tty/{mod.rs,port.rs,terminal.rs,discipline.rs}`、
`driver/serial/ns16550a/{mod.rs,port.rs,regs.rs}`、`conf/.defconfig`、
`scripts/xtask/src/config/kconfig.rs`与transaction；`endpoint.rs`只可为worker组合的最小同owner骨架创建，
不得提前publish。

**Review / validation floor：** `git diff --check`、`just fmt kernel --check`、四项kconfig default/non-zero tests；
依次执行`just conf switch qemu-virt-rv64-pretest && just build`、
`just conf switch qemu-virt-la64-pretest && just build`，最后切回入口platform并再次`just build`。KUnit必须覆盖两层predicate、
完整transform-token进度、无echo/input丢失、guard-out port/wake和stop/join；source audit证明first-drain sink为零、
Terminal/port/endpoint不持worker handle、没有周期polling和guard内allocation/TX/Latch drop。

**停止条件：** 需要修改KThread/Event/wait/scheduler、让Terminal直接持port/worker、用第二个TX worker或shadow
queue、无法避免strong cycle、无法让partial port TX保持用户byte进度诚实，或capacity pressure只能静默丢
已接受data。Checkpoint 1关闭不自动激活Checkpoint 2。

#### Checkpoint 2 — TTY FileOps、asm-generic UAPI 与 readiness

**目的与交付：** 新建TTY-owned FileOps，接入blocking/nonblocking read/write、poll/select、termios/winsize
ioctl和真实drain/revalidation；新增`anemone-abi::tty::linux`及`anemone-rs`最小raw/typed wrapper。用KUnit和
ABI tests闭合7.2--7.4的input/output/ioctl/errno/layout/readiness matrix；endpoint仍unpublished，boot行为不变。

**允许写集：** Checkpoint 1已建立的TTY/port文件、计划新建`device/tty/file.rs`、
`anemone-kernel/src/fs/file.rs`中唯一的immutable typed private-data accessor、
`anemone-abi/src/{lib.rs,tty.rs}`、`anemone-rs/src/{sys/linux.rs,os/linux.rs}`与transaction。generic
`fs/iomux.rs`和syscall adapters是validation-only；除上述accessor外若现有ctx/poll contract不足，必须停止上报，
不得在本checkpoint继续扩大generic surface。accessor只返回owner安装的具体`&T`，不得暴露mutable `AnyOpaque`、Task、
fd table或VFS guard。

**Review / validation floor：** `git diff --check`、`just fmt --check`，依次执行
`just conf switch qemu-virt-rv64-pretest && just build`、`just conf switch qemu-virt-la64-pretest && just build`，
最后恢复入口platform；ABI tests核对
NCCS/layout/ioctl constants，fake Terminal/File KUnit覆盖canonical boundary、empty VEOF、nonblock/short progress、
binary write、TCSETS三种ordering、unsupported candidate不发布、winsize和register-plus-recheck race。review确认
`O_NONBLOCK`来自operation ctx、copyout无rollback shadow、trigger在guard外drop/execute、unknown ioctl为ENOTTY。
另审查typed accessor只完成既有`OpenedFile` private-data producer/consumer pair，不被TTY用于取得owner外状态。

**停止条件：** 必须把Task/caller放入generic FileOps、缓存per-file termios/nonblock、用raw RX或wake count作为
readable、success-stub unsupported bits、`TCSETSW/F`在drain前发布snapshot，或需要Stage 3 relation/Signal才能
使Stage 2 policy自洽。Checkpoint 2关闭不自动激活Checkpoint 3。

#### Checkpoint 3 — deterministic endpoint、console 与boot publication

**目的与交付：** 完成sorted `ttyS<N>` mapping、TTY devfs provider、console selected-terminal identity和
console-owned`/dev/console`，在显式post-Late finalize中单向publish，并把boot fd 0/1/2替换为real Terminal files。
用KUnit/source audit验证multi-port顺序、duplicate/overflow、chosen/fallback selection、pre-publish failure与
shared snapshot；本checkpoint不加入userspace harness或执行cutover。

**允许写集：** `device/tty/{mod.rs,port.rs,terminal.rs,discipline.rs,file.rs,endpoint.rs}`、
`driver/serial/ns16550a/{mod.rs,port.rs}`、`device/{console.rs,devnum.rs}`、`main.rs`与transaction。
`fs/devfs/`只读；若publish原语不足，停止报告owner/API/write-set扩展，不能用generic CharDev或私有devfs旁路。

**Review / validation floor：** `git diff --check`、`just fmt kernel --check`，执行
`just conf switch qemu-virt-rv64-pretest && just build`并运行定向KUnit。本轮用户明确处置LA64 build/runtime
为Not Run；C3不cutover，RV64证据不得外推为LA64 compile/runtime proof。该validation disposition只调整
本checkpoint证据安排，不改变R0 target、owner、ABI或`TTY-DATA-CUTOVER`组成。
source audit证明编号不依赖probe顺序、selected truth只在console、全部fallible prepare早于首个publish、published
ops长期稳定、boot files不是anonymous console、`/dev/console`不委托Terminal、raw 234与旧boot open helper无
production caller。runtime留到Checkpoint 4，build/KUnit不冒充devfs或BusyBox proof。

**停止条件：** 需要runtime unpublish/编号复用、TTY复制selected-console state、TTY接管console fan-out、
`/dev/console`必须返回Terminal、首个publish后仍有可恢复fallible步骤、或boot只能回退anonymous EOF stdin。
Checkpoint 3关闭不自动激活Checkpoint 4。

#### Checkpoint 4 — userspace acceptance 与 `TTY-DATA-CUTOVER`

**目的与交付：** 新增单一`tty-test` app及其最小launcher、一份RV64 TTY rootfs manifest、稳定BusyBox staging说明和
RV64 repository wrapper；完成自动data-plane/termios/readiness/devfs/boot matrix、BusyBox `stty`/`vi`
smoke、人工vi checklist、全树bypass/manifest audit。全部proof满足后，原子创建TTY current contract并执行
`TTY-DATA-CUTOVER`，同步RFC/transaction/navigation；不进入Stage 3 resolution或实现。

**允许写集：** `anemone-apps/tty-test/**`、`anemone-abi/src/{lib.rs,tty.rs}`、
`anemone-rs/src/{sys/linux.rs,os/linux.rs}`、`conf/rootfs/tty-acceptance-rv64.toml`、
`conf/rootfs/tty-acceptance.md`、`scripts/run-tty-test-rv64.sh`；本Stage全部kernel文件仅允许修复本阶段
matrix暴露的in-target缺陷。cutover成功时另允许7.7列出的contract与lifecycle docs。不得修改pretest manifests、
`user-test`/LTP profile、external BusyBox、测试盘master或个人mount。

**Harness contract：** wrapper显式接收`--busybox <artifact>`、`--sdcard <master>`、
`--mode auto|vi`和可选`--log`；复制前核对ELF arch/static identity与SHA-256，只复制到
`build/tty-acceptance/staging/<arch>/busybox`，从不原地修改。version和目标applets优先用host qemu-user核对；
host无对应emulator时由guest launcher在任何acceptance case前执行并打印结果，缺失即fail closed。rootfs
manifest只引用该稳定ignored staging路径；wrapper复制显式测试盘到worktree-local QEMU文件，构建独立rootfs、
按平台所需路径接线、`just build`并调用`just xtask qemu`。不得在manifest/script/docs中硬编码个人`etc/`路径
或修改默认pretest init链。最小launcher作为该rootfs的init，先挂载devfs、验证boot fd，再运行自动matrix或
BusyBox vi，负责child reap、termios恢复、结果打印和正常关机；它不建立Stage 3 session/relation流程。

**自动matrix：** RV64必须验证`/dev/ttyS0`为4:64、`/dev/console`为5:1、boot三fd为real shared Terminal；
canonical incomplete/newline/erase/kill/VEOF/short-record、ICRNL、noncanonical VMIN1/VTIME0、nonblock EAGAIN、
binary write、OPOST/ONLCR、shared termios/winsize、TCSETSW/F drain/flush、unsupported update rollback、poll/select
readable/writable和unknown ioctl。`tty-test`逐项输出稳定PASS/FAIL并自动关机；BusyBox `stty`至少完成read/
round-trip，`vi` auto smoke必须进入raw mode、读winsize、写入/退出并恢复原termios。

**人工vi checklist：** 使用同一已记录hash的RV64 artifact启动`--mode vi`；记录commit/platform/hash/log，依次
确认80x24显示、进入vi、插入含两行文本、Backspace删除一个字符、Esc后`:wq`、launcher重读文件内容正确；
确认无双重echo、无每键卡顿、换行没有阶梯/重复CR、退出后canonical+echo snapshot恢复，再输入一行并用
`VERASE/VKILL/VEOF`检查可见编辑与read boundary。人工观察不能替代自动matrix；LA64 compile/runtime按R1为Not Run。

**Review / validation floor：** `git diff --check`、`just fmt --check`、
`just app build tty-test --arch riscv64`；随后运行
`./scripts/run-tty-test-rv64.sh --busybox <rv64-busybox> --sdcard <rv64-master> --mode auto --log
build/tty-stage2-rv64.log`，再以同一RV64输入执行`--mode vi`人工checklist。一份rootfs和kernel build由wrapper
按7.5 harness contract完成；最后执行`mdbook build docs`和contract/link/manifest/
bypass audit。必须区分source/KUnit/build、
agent-run QEMU、user-run vi与Not Run；RV64自动matrix、RV64 vi、artifact identity或proof任一不足时不cutover。
LA64 compile/runtime明确Not Run且不作为R1 cutover blocker，RV64证据不得外推为LA64 proof。

**停止条件：** wrapper必须引用个人path、artifact非目标arch/static或缺applet、launcher/test含kernel后门/
success fallback、默认pretest/LTP flow被修改、只靠shell prompt或人工“可用”关闭、in-target failure被改名为
limitation，或current contract无法作为一个unit原子建立。Checkpoint 4关闭不自动进入Stage 3。

### 7.6 Stage-wide审计、可观测性、cutover与退出

Stage-wide审计：

- 全树审计Stage 1 discard/first-drain sink、anonymous boot stdio caller、NS16550A raw 234/CharDev、
  `/dev/ttyS*`/`/dev/console`重复publisher、probe-order numbering、console RX、direct Terminal-to-port write、
  output/readiness shadow、polling watchdog、termios/ioctl success stub和Stage 3 relation/signal fallback。
- 所有Terminal diagnostic字段明确不参与predicate/ordering/state transition。至少记录canonical input overflow/
  flush、unsupported termios分类、no-foreground ISIG/SIGWINCH、output backpressure/partial port progress、
  drain waits、endpoint mapping/publish和trigger detach；每字节、每IRQ或无限重复日志禁止。
- correctness assertion至少覆盖record boundary与queue accounting、完整transform token、read/poll predicate一致、
  setter generation/revalidation、trigger register-plus-recheck、worker-local batch唯一ownership、notifier无strong cycle、
  sorted mapping/name/devnum一致和publish前全部fallible prepare完成。cleanup先撤visibility/stop再assert。

Contract cutover：

- `TTY-DATA-CUTOVER`只在四个checkpoint均关闭、RV64自动matrix与RV64人工vi达到floor后执行；它原子创建
  `docs/src/contracts/tty/{index.md,data-plane.md}`中的Active `TTY-PORT-001`、`TTY-TERM-001`、
  `TTY-INPUT-001`、`TTY-OUTPUT-001`和`TTY-ENDPOINT-001`，并同步`contracts.md`、`SUMMARY.md`、RFC入口、
  transaction和双周devlog。任一ID证据不足时整个unit保持Not Cut Over，不创建部分current truth。
- cutover后`TTY-REL-001`、`TTY-JOBCTL-001`、`TTY-LIFE-001`和`TTY-ABI-001`仍为Not Cut Over；RFC保持
  Accepted for Implementation。Stage 3只可在Stage 2关闭后的独立`2 -> 3` resolution gate中解析。

Stage-wide停止条件：

- natural implementation越过7.7 exact manifest、generic VFS/devfs/console API不足、或新增owner/public API/
  shared-contract/ABI/visible-semantics/acceptance变化时，按扩展或Target Renegotiation Gate停止；不得用
  callback、CharDev、global state或test-only path绕过。
- output queue/worker路线无法同时满足用户byte progress、echo、drain、writable与引用生命周期，先记录真实
  cost和partial-code disposition，不自行降级为同步partial transform或best-effort echo。

退出条件：

- 四个checkpoint逐项activation/review/validation/closure，所有bridge disposition和分层证据追加transaction；
- production endpoint与7.2--7.4一致，Stage 1 sink/anonymous boot stdio/raw 234/所有listed bypass为零，
  multi-open shared truth、readiness、drain、publication和BusyBox vi行为达到本节matrix；
- `TTY-DATA-CUTOVER`作为一个unit成功并可由current contract定位，或明确保持Not Cut Over且Stage 2不关闭；
- Stage 2关闭后立即停止，不自动解析Stage 3。

### 7.7 Resolved Write Set Manifest

允许tracked source写入：

- `anemone-kernel/src/device/tty/{mod.rs,port.rs}`，新建
  `anemone-kernel/src/device/tty/{terminal.rs,discipline.rs,file.rs,endpoint.rs}`；
- `anemone-kernel/src/fs/file.rs`只允许Checkpoint 2增加immutable typed、crate-internal file private-data
  accessor；不得改变FileOps签名、operation ctx、status owner、cursor/copy semantics或其它backend行为；
- `anemone-kernel/src/driver/serial/ns16550a/{mod.rs,port.rs,regs.rs}`，只用于line/idle capability、console
  identity registration和本阶段transport/KUnit适配；
- `anemone-kernel/src/device/{console.rs,devnum.rs}`与`anemone-kernel/src/main.rs`，只用于7.4的selected identity、
  console node、post-Late finalize和real boot stdio；
- `conf/.defconfig`、`scripts/xtask/src/config/kconfig.rs`，只加入7.2四项参数、non-zero/default/parser tests；
  root `kconfig`与generated defs只能由repository入口生成，不手改、不作为source提交；
- `anemone-abi/src/{lib.rs,tty.rs}`、`anemone-rs/src/{sys/linux.rs,os/linux.rs}`；
- 新建`anemone-apps/tty-test/**`、`conf/rootfs/tty-acceptance-rv64.toml`、
  `conf/rootfs/tty-acceptance.md`、`scripts/run-tty-test-rv64.sh`；
- `docs/src/contracts/tty/{index.md,data-plane.md}`、`docs/src/contracts.md`和`docs/src/SUMMARY.md`只在
  `TTY-DATA-CUTOVER`成功时原子创建/导航；
- 本文只用于获准的Stage 2 route correction/manifest expansion；transaction只追加checkpoint、review、
  validation、artifact provenance、cutover和closure事实；RFC入口、`rfcs.md`与当前双周devlog只同步lifecycle。

Validation-only inputs：

- `anemone-kernel/src/fs/{iomux.rs,devfs/,api/read_write/,api/iomux/}`、`device/char/`、
  `device/discovery/open_firmware/`、kthread/Event/Latch/wait/scheduler、task topology/Signal/jobctl；
- Stage 1全部diff/source/log、current contracts、register、RFC invariants/tracking/backgrounds与Linux 6.6.32
  asm-generic UAPI/TTY source；
- BusyBox source/config、用户提供的RV64 artifact、测试盘master和mount均只读；只把版本、架构、hash、
  applet/runtime核对结果写入tracked provenance，不写个人路径；
- `Justfile`、live xtask app/rootfs/qemu/config owners、platform/pretest configs与现有end-to-end wrappers。

不允许触碰：

- generic VFS/FileOps implementation（除上述typed accessor）、iomux/devfs implementation、CharDev、IRQ core、
  kthread/Event/Latch/wait/scheduler、task topology/Session/ProcessGroup/Signal/jobctl、pretest rootfs、
  `user-test`/LTP profile、register/current limitations；
- Stage 3/4 relation、`/dev/tty`、signal/access/lifecycle实现；alpha worktree、external source、BusyBox原件和
  测试盘master。

实现责任：

- implementer一次只激活一个checkpoint并按7.5子集追加transaction；reviewer依次审查discipline/worker、
  FileOps/UAPI/readiness、identity/publication/boot和userspace/cutover；integrator在Checkpoint 4执行全manifest、
  RV64 acceptance、artifact、bypass与contract atomicity审计。任何角色都不能把Ready或长期scope envelope当作执行授权。

## 8. Stage 3 Ready：controlling relation 与 caller/topology vertical slice

阶段成熟度：

- `Closed`。本节保留当时冻结的完整交付、route、ABI matrix、验证与manifest；实际执行与关闭证据见transaction。
- 本阶段不再拆分checkpoint。relation registry、caller/topology decision、`/dev/tty`/ioctl和exit cleanup
  是一个不可再拆的vertical slice；拆开会引入对外可见的无条件成功、临时errno或双重relation truth。
- Stage 3只验证Gate P2 candidate且已关闭，不执行`TTY-JOBCTL-CUTOVER`。四个relation/job-control ID继续Not Cut Over。

### 8.1 前置条件与受保护边界

- Stage 2已经Closed，`TTY-DATA-CUTOVER` Effective；live endpoint、real boot stdio、Terminal FileOps与
  `tty-test` RV64 harness均来自`4f15e23d`，不能退回anonymous console或另建测试入口。
- 保护`TTY-REL-001`、`TTY-JOBCTL-001`、`TTY-LIFE-001`和全部Preserve contract IDs，特别是
  `SIGNAL-PENDING-001/002`、`SIGNAL-ACTION-001/002`、`PGRP-SIGNAL-001/002`、`JOBCTL-*`与`TASK-LIFE-*`。
- `Session`/`ProcessGroup`继续唯一拥有membership；Signal继续拥有mask/disposition与occurrence；
  ThreadGroup继续唯一拥有terminal lifecycle与job-control phase。TTY只拥有relation、foreground selector
  和terminal-side decision orchestration。
- numeric SID/PGID只在UAPI边界使用。跨lock/lifecycle保存的session、process-group、caller和terminal
  identity必须是task/TTY owner提供的opaque stable capability，并以owner lookup + identity equality重验。
- last close不拆relation；detach/exit不销毁Terminal、`/dev/ttyS<N>`或`/dev/console`。relation-disassociation
  `SIGHUP`/`SIGCONT`、orphaned-pgrp policy、hardware hangup、backend fatal、`TOSTOP`与其它terminal-modifying
  matrix保持非目标。

### 8.2 单一路线

**Caller与task-side capability：** 在`task/jobctl`下增加TTY专用的窄caller/session/process-group
capability。`/dev/tty` open和TTY ioctl在同步entry即时从`get_current_task()`构造短命caller；capability内部可以
短暂持有live task/thread-group引用，但完整`Task`、Signal state和topology guard不得存入`Terminal`、opened file或
relation。live `FileIoCtx`、`IoctlCtx`和`DevfsNodeOps`已经足够，本阶段不修改generic VFS ctx、devfs或CharDev。

task-side API负责：验证caller仍是live user ThreadGroup、解析并重验stable Session/ProcessGroup identity、判断
session leader、验证candidate foreground group仍属于同一session，以及把current caller的`SIGTTOU` mask/shared
disposition分类为blocked/ignored或actionable。Signal owner另提供一个只返回该分类的窄查询；TTY不得读取或缓存
Signal内部字段。

**Relation owner：** 新建`device::tty::relation`，以单个registry guard拥有全部relation entry；一个entry只保存
同一份published endpoint、stable controlling-session capability、optional stable foreground process-group capability
和checked generation。registry在boot prepare按published endpoint数量一次性预留容量；runtime acquire/remove/
foreground replacement不得在guard内扩容、复杂drop或调用task/Signal/Event。session与terminal uniqueness均在
同一个guard内检查，不在`Session`或`Terminal`另存binding/PGID副本。

所有跨owner操作使用同一循环：relation guard内取得immutable snapshot并释放guard；task/topology/Signal owner
执行decision或effect；mutation回到relation owner，以endpoint + session identity + generation重验后commit，失配则
重试或返回当前owner决定的errno。generation使用checked单调推进；remove/replacement取出旧capability后先释放guard，
再drop。foreground group已退出时，下一次snapshot/revalidation只清空selector并保留relation；`TIOCGPGRP`投影0，
合法same-session caller仍可安装新的foreground group。

**Publication与open：** 在现有`prepare_system_boot()`的首个devfs commit之前准备TTY-owned major 5/minor 0
`/dev/tty` provider与空relation registry；沿用当前console -> TTY单向publish transaction，不修改devfs或
`/dev/console` owner。`/dev/tty` open从同步caller解析live controlling relation并返回该published endpoint的正常
Terminal file；没有relation、relation已撤销或caller session不匹配时返回`ENXIO`。`/dev/ttyS<N>`和boot files
仍直接打开endpoint，打开本身不会隐式取得controlling terminal，`O_NOCTTY`继续不需要新增TTY特判。

**Ioctl与完整`TIOCSPGRP`非orphan core：** `anemone-abi`补齐asm-generic五个job-control ioctl常量，TTY
FileOps在同一relation route实现`TIOCSCTTY`、`TIOCNOTTY`、`TIOCGSID`、`TIOCGPGRP`和`TIOCSPGRP`。
Stage 3不保留“先无条件成功、Stage 4再修”的临时路径：

- `TIOCSCTTY`先拒绝任意非零arg为`EPERM`；`arg=0`只允许live session leader以可读file建立relation，初始
  foreground为caller当前process group。同一session/terminal的`arg=0`重复调用在file-mode检查前幂等成功；
  非leader、session已控制其它terminal、terminal已被其它live session控制或首次acquire使用不可读file返回
  `EPERM`。`arg=1` privileged steal不伪造支持。
- `TIOCGSID`/`TIOCGPGRP`只允许当前caller session控制该terminal；否则`ENOTTY`。前者写回SID，后者写回live
  foreground PGID或0；copyout fault保持`EFAULT`且不改变relation。
- `TIOCSPGRP`先按当前relation snapshot执行background decision，再读取signed PGID并验证candidate。负值
  `EINVAL`，不存在/已失效`ESRCH`，不同session `EPERM`，caller不控制该terminal `ENOTTY`。foreground caller或
  background + blocked/ignored `SIGTTOU`继续；background + actionable `SIGTTOU`由Signal/process-group owner向
  caller process group发布kernel-origin occurrence并返回`RestartSyscall::Idempotent`，不得提前commit foreground。
  candidate通过后才回relation owner按generation提交。orphaned-pgrp errno/effect仍不在本阶段或首版target内，
  Stage 3不得为它修改topology owner或把未验证行为写成支持。
- `TIOCNOTTY`要求caller控制该terminal；首版只有session leader可以撤销，non-leader返回`EPERM`，无relation/
  wrong terminal返回`ENOTTY`。成功只撤relation与foreground selector，不发送`SIGHUP`/`SIGCONT`。

上述ABI取舍必须在command dispatch和narrow decision API旁写关键注释；unsupported steal/non-leader detach等
拒绝路径保留限频/非热路径诊断，不能用success stub或静默状态丢弃。

**Lifecycle handoff：** task lifecycle不保存relation truth，也不引入callback registry或generic observer。
`task/api/exit`只在session-leader ThreadGroup第一次`Alive -> Exiting`以及未经过`exit_group`的最后member退出路径，
于ThreadGroup/topology guard外向TTY传递opaque session-leader identity。TTY hook幂等移除匹配relation，先撤销
discoverability，再在guard外drop；后续last-member重复通知是no-op。acquire与exit的窄竞态仍由每次relation lookup/
mutation对session-leader lifecycle的owner revalidation闭合：即使exit通知与尚未commit的acquire交错，旧relation
也不能被`/dev/tty`或foreground mutation重新发现，并会在下一次owner操作惰性清除。

### 8.3 审计、可观测性与review

- relation/source audit必须证明：每个session/terminal至多一个entry；foreground capability属于同一session；
  generation不回绕；所有Arc/drop、Signal/Event和task/topology调用均在relation guard外；无Session/Terminal双cache。
- lock graph固定为“TTY snapshot -> guards-out task/Signal decision -> TTY generation commit”；不允许relation guard
  嵌套topology、ThreadGroup、Signal、Terminal、port或devfs guard，也不允许task lifecycle持owner guard调用TTY。
- source audit确认`FileIoCtx`、`IoctlCtx`、`DevfsNodeOps`、generic devfs/CharDev、process-group/session core、
  Signal pending/generation、ThreadGroup jobctl state与scheduler/wait均无diff。
- 只在relation acquire、foreground commit、explicit/exit detach和unsupported steal等稀有边界记录sid/pgid/
  generation与结果；不对每次`/dev/tty` lookup或普通ioctl成功刷日志。diagnostic字段/日志不得驱动行为。
- review按relation owner/identity、caller+Signal decision、lifecycle、UAPI/errno、userspace oracle五个切面一次完成；
  不为这些切面建立checkpoint。最终必须为0 Apollyon / 0 Keter / 0 Euclid，Safe只记录不扩scope。

### 8.4 RV64验证

tracked验证：

1. `git diff --check`；
2. `just fmt --check`；
3. active `qemu-virt-rv64-pretest`下`just build`；
4. `just app build tty-test --arch riscv64`；
5. 复用现有repository wrapper与rootfs，不新增mode/manifest/launcher：
   `./scripts/run-tty-test-rv64.sh --busybox <rv64-busybox> --sdcard <rv64-sdcard-master> --mode auto --log build/tty-stage3-rv64.log`。

`tty-test`在既有auto matrix后追加自动relation组，至少覆盖：`/dev/tty`为5:0且无relation时`ENXIO`；普通open不
隐式attach；session leader acquisition与same-relation幂等；nonleader/occupied/nonzero-arg拒绝；`TIOCGSID`/
`TIOCGPGRP`；foreground allow；background blocked与ignored `SIGTTOU`两条reclaim；actionable `SIGTTOU`使
background helper被`wait4(WUNTRACED)`观察为Stopped且foreground未提前改变，随后由parent kill/reap；candidate
负值/不存在/其它session errno；nonleader `TIOCNOTTY`拒绝；leader detach后`/dev/tty`失效并可重新acquire；
session leader exit后另一个session可取得同一terminal。每个child都必须有界reap，失败路径不得留下stopped child。

同一wrapper必须继续通过既有data-plane、BusyBox stty/vi与host byte oracle，证明Stage 3没有退化五个Active
`TTY-DATA-*` contract ID。日志记录commit/candidate、platform、BusyBox/rootfs/kernel hash、KUnit总数、逐case
PASS/FAIL和正常关机。只测试RV64；LA64 compile/runtime与hardware明确Not Run，RV64结果不得外推。

### 8.5 停止与退出条件

立即停止并上报manifest expansion或Target Renegotiation：

- 必须修改generic VFS ctx、devfs/CharDev、task topology membership结构、Signal pending/generation、ThreadGroup
  jobctl truth、scheduler/wait、architecture代码或现有data-plane owner才能继续；
- 需要把relation/foreground同时缓存到Session/Terminal，保存裸SID/PGID跨lifecycle，或持relation guard调用
  topology/Signal/Event；
- exit cleanup只能通过generic callback framework、第二lifecycle truth、轮询或relation-disassociation signals闭合；
- `/dev/tty`不能在现有single-way boot transaction中完成fallible prepare，或runtime test需要新rootfs/wrapper模式；
- 非orphan `TIOCSPGRP`三分支、errno、restart/no-mutation或generation revalidation无法在accepted target内成立；
- 任一Active data-plane contract回归、RV64 wrapper未正常关机，或review仍有Apollyon/Keter/Euclid。

退出条件：全部manifest diff与ABI matrix闭合；RV64 build/app/KUnit/auto relation+data regression通过；eager exit hook与
lazy missed-race revalidation均经source/runtime审计；无临时bridge、success stub、双relation truth或新register缺口。
Stage 3随后记为Closed，但`TTY-JOBCTL-CUTOVER`仍Not Cut Over，RFC保持Accepted for Implementation并立即停止；
不得自动解析或进入Stage 4。

### 8.6 Resolved Write Set Manifest

允许实现文件：

- `anemone-abi/src/tty.rs`：只增加asm-generic job-control ioctl常量与layout/value测试；
- `anemone-kernel/src/device/tty/{mod.rs,endpoint.rs,file.rs}`、新建
  `anemone-kernel/src/device/tty/relation.rs`：relation owner、5:0 provider、FileOps ioctl与owner-local测试；
- `anemone-kernel/src/task/jobctl/{mod.rs,terminal.rs}`：短命caller与stable session/process-group capability、
  topology decision/revalidation；
- `anemone-kernel/src/task/sig/{mod.rs,terminal.rs}`：只提供current mask/shared disposition的TTY窄分类，不修改
  pending/generation/delivery truth；
- `anemone-kernel/src/task/api/exit/mod.rs`：只增加guards-out session-leader identity handoff；
- `anemone-rs/src/{sys/linux.rs,os/linux.rs}`：补齐现有syscall/ioctl的最小safe wrappers；
- `anemone-apps/tty-test/src/main.rs`：在既有auto mode追加Stage 3自动relation/errno/lifecycle matrix。

允许文档写回：

- `docs/src/devlog/transactions/2026-07-23-tty-subsystem.md`追加activation、review、RV64 evidence与closure/stop；
- 本文只在获准route correction/manifest expansion时更新；RFC入口、`docs/src/rfcs.md`、transaction index与当前
  双周devlog只在Stage 3 lifecycle变化时同步。Stage 3不修改current contracts、register、invariants或tracking issues；
  若真实finding要求这些surface，先按正常RFC review/Target Renegotiation Gate停止。

Validation-only inputs：live Stage 2 diff/log、TTY data-plane与全部Preserve contracts、register、Linux 6.6.32
`tty_jobctrl.c`/asm-generic UAPI、BusyBox source/artifact、现有rootfs/wrapper/Justfile/xtask owners。external artifact与
测试盘master保持只读；公共文档不记录个人路径。

明确不允许：`anemone-kernel/src/fs/**`、`device/char/**`、`device/tty/{terminal.rs,discipline.rs,port.rs}`、
`task/topology/**`、`task/jobctl/{group.rs,report.rs,user_entry.rs}`、`task/sig/{generation.rs,pending.rs,delivery.rs}`、
`main.rs`、driver/console/IRQ/kthread/Event/wait/scheduler、rootfs manifest、wrapper、pretest/user-test/LTP、architecture、
current contracts、register与Stage 4实现。越过这些边界必须先扩展，不得用相邻“顺手修复”掩盖。

## 9. Stage 4 Closed：terminal job control、完整验收与 `TTY-JOBCTL-CUTOVER`

阶段成熟度：

- `Closed`。本节保留Stage 4进入实现前冻结的Ready交付、单一路线、signal/access matrix、RV64验证、cutover与
  resolved manifest，作为历史计划authority；实际授权、扩展、证据与关闭见第12.3和12.4节。
- 本阶段不拆checkpoint。三条terminal effect都复用Stage 3 relation snapshot与同一个process-group Signal入口；
  `tty-test`、BusyBox ash验收和current-contract cutover是这条最终路径的proof与收口，不建立第二套实现owner。
  冻结计划规定自动floor已过但用户人工ash证据尚缺时保持`Active / Awaiting User Evidence`且不另建验收stage；
  该中间状态已由第12.4节的user evidence与原子cutover终结。

### 9.1 前置条件与受保护边界

- Stage 3已经Closed；`5bf8024a`中的single-registry relation、stable session/process-group capability、
  checked generation、caller-relative`/dev/tty`、五个ioctl与detach/exit cleanup是live输入，不重新设计。
- `TTY-DATA-CUTOVER` Effective；239项KUnit、`TTYTEST:SUMMARY:PASS:34`、Stage 3 relation matrix与既有
  data-plane/BusyBox vi RV64回归已经通过。LA64 compile/runtime与hardware按R1保持Not Run。
- 保护`TTY-REL-001`、`TTY-JOBCTL-001`、`TTY-LIFE-001`、`TTY-ABI-001`、
  `TTY-LOCAL-001/002/005/006`和全部Preserve IDs。TTY只产生terminal-side decision/request；task topology、
  Signal与ThreadGroup jobctl分别继续拥有membership、occurrence/action和stop/continue/report truth。
- `TIOCSPGRP`继续使用Stage 3唯一foreground mutation路线；Stage 4不得复制foreground selector、从Signal/wait
  结果反推relation，或为ash增加global PGID、opener identity、success stub和kernel test backdoor。
- `TOSTOP` write、其它terminal-modifying`SIGTTOU`、orphaned-pgrp errno/effect、relation-disassociation
  `SIGHUP`/`SIGCONT`、runtime line change、hardware hangup/backend fatal、PTY与`/proc/<tgid>/stat` TTY字段保持
  target之外。Stage 4不因最终cutover顺手补这些corner。

### 9.2 单一实现路线

**Control-character effect：** `discipline.rs`继续在Terminal guard内完成`VINTR/VQUIT/VSUSP`识别、已接受的
input/output flush与echo，但不再只返回无target诊断。receive结果携带一个窄、无分配的terminal signal request，
分别映射到`SIGINT/SIGQUIT/SIGTSTP`；worker在确认byte已消费并释放Terminal guard后，立即通过endpoint relation
snapshot取得live foreground capability，再调用task-side窄process-group signal API。request不进入持久队列，
worker wake/notification也不成为signal truth。relation或foreground不存在/失效时只累计限频诊断，不回退到current
task、最近reader或全局PGID；并发relation切换允许effect线性化到已重验的合法前态。

**Background read：** `tty_read`在每次可能消费input之前、以及blocking wait返回后的下一轮，使用同步current caller
和endpoint relation snapshot执行同一套owner revalidation。caller不受该terminal控制或位于foreground时继续；
同session background caller的`SIGTTIN`为actionable时，向caller当前process group生成kernel-origin occurrence并
返回`RestartSyscall::Idempotent`，本次不得消费input；blocked/ignored时按Linux/POSIX可见语义返回`EIO`，不得让
background reader窃取input。没有live foreground selector时同样fail closed为`EIO`。首版不增加orphaned-pgrp检测；
该延期边界保持显式，不能用它跳过上述非orphan路径。

Signal owner把现有只服务`SIGTTOU`的mask/disposition查询收窄泛化为只接受`SIGTTIN/SIGTTOU`的TTY job-control
查询；它仍只返回blocked-or-ignored/actionable分类，不暴露mask或disposition。task-side capability复用一个受限的
kernel-origin process-group signal helper发送`SIGINT/SIGQUIT/SIGTSTP/SIGTTIN/SIGTTOU/SIGWINCH`，每次发送前重验
stable group/session identity；不修改Signal pending/generation/delivery或ProcessGroup membership实现。

**Winsize effect：** Terminal的winsize setter返回snapshot是否真实变化；`TIOCSWINSZ`只在成功提交不同值后、
Terminal guard外对当时live foreground group生成一次`SIGWINCH`。相同值不生成重复signal；无relation/foreground
只记录诊断。winsize仍唯一归Terminal，relation只提供target snapshot，不缓存尺寸或effect结果。

**Userspace与ash验收：** 不新增app、rootfs manifest、通用launcher或测试框架。现有`tty-test` auto mode追加
foreground control、winsize和background-read定向cases，并在全部定向case后以子进程执行显式
`setsid -> TIOCSCTTY(arg=0) -> foreground -> BusyBox ash -i`序列。现有RV64 wrapper继续驱动serial byte输入，
自动ash smoke至少证明没有`job control turned off`降级、Ctrl-Z产生Stopped job、`jobs`可见、`fg`收回foreground、
Ctrl-C终止foreground job并返回shell、shell exit后launcher可reap且relation cleanup允许复用。既有34项matrix、
BusyBox stty/vi与host byte oracle全部继续回归。

wrapper只增加一个`jobctl`人工mode并复用同一rootfs/staging/BusyBox；`conf/rootfs/tty-acceptance.md`冻结以下RV64
checklist：启动ash无降级提示；foreground job Ctrl-C返回prompt；执行
`Ctrl-Z -> jobs -> fg -> Ctrl-Z -> bg -> jobs -> fg -> Ctrl-C`两轮交接；background `cat`因`SIGTTIN`停止并可由
`fg`恢复；最后退出ash并观察launcher PASS与正常关机。命令使用现有BusyBox applet和ash builtin，不增加rootfs
symlink或外部脚本。Stage 2用户vi证据不冒充本checklist，但同一auto run中的vi回归无需重复人工执行。

### 9.3 审计、可观测性与review

- source/lock audit证明Terminal guard只形成effect request，relation guard只复制stable snapshot；Signal、Event、
  task topology、process-group broadcast、echo TX和复杂drop全部guards-out。request、counter和日志均不是行为truth。
- 每个terminal signal必须来自snapshot中live、same-session foreground或caller process group；source audit核对
  `SIGINT/SIGQUIT/SIGTSTP/SIGTTIN/SIGWINCH`映射、相同winsize suppression、read-before-consume与wait后重验。
- 审计Stage 2留下的`no_foreground_isig/no_foreground_winsize`relationless bridge：计数器可保留为纯诊断，
  但production relation存在时必须走真实effect；不得保留“总是consume但从不signal”的第二条路径。
- 全树搜索并分类global/recent/opener PGID、direct ThreadGroup stop、TTY内pending occurrence、relation/foreground
  duplicate、polling watchdog、success stub与test-only injection；允许项必须有owner和target依据。
- 只在缺失/失效foreground、blocked/ignored background read和unsupported延期边界做限频诊断；普通control char、
  read和winsize成功不刷日志。最终review为0 Apollyon / 0 Keter / 0 Euclid，Safe不扩大scope。

### 9.4 RV64验证与用户证据

tracked验证：

1. `git diff --check`；
2. `just fmt --check`；
3. active `qemu-virt-rv64-pretest`下`just build`；
4. `just app build tty-test --arch riscv64`；
5. `mdbook build docs`；
6. agent-run自动matrix：
   `./scripts/run-tty-test-rv64.sh --busybox <rv64-busybox> --sdcard <rv64-sdcard-master> --mode auto --log build/tty-stage4-rv64.log`；
7. user-run人工ash checklist：同一commit、platform、BusyBox与测试盘，使用
   `--mode jobctl --log build/tty-stage4-rv64-jobctl.log`。

自动定向matrix至少覆盖：`VINTR -> SIGINT`、`VQUIT -> SIGQUIT`、`VSUSP -> SIGTSTP`只命中live foreground
process group；changed winsize只产生一次`SIGWINCH`且same-value不重复；background actionable`SIGTTIN`使helper被
`wait4(WUNTRACED)`观察为Stopped且input未消费，blocked/ignored为`EIO`；detach/exit后旧relation不再产生effect；
Stage 3全部relation/`TIOCSPGRP`case与五个Active data-plane ID不回归。每个child有界reap，失败路径不得留下
stopped/background child。日志记录base/candidate、platform、BusyBox/rootfs/kernel hash、KUnit总数、逐case、
ash host oracle、正常关机和agent/user provenance。

只测试RV64。LA64 compile/runtime、hardware和LTP均明确Not Run；RV64证据不得外推为这些层级。用户人工checklist
只证明无法稳定自动判定的ash交互序列，不能替代自动signal/access matrix、KUnit、source audit或contract atomicity。

### 9.5 停止、cutover与退出条件

立即停止并上报manifest expansion或Target Renegotiation：

- 必须修改generic VFS ctx、task topology/ProcessGroup membership、Signal pending/generation/delivery、ThreadGroup
  jobctl group/report/user-entry、scheduler/wait、driver/console/IRQ、architecture或rootfs manifest才能继续；
- 必须建立persistent effect queue、第二relation/foreground cache、完整Task持久引用、global/opener/recent-reader
  fallback，或持Terminal/relation guard进入Signal/topology；
- control character无法在consume后guards-out生成准确foreground signal，background read无法在consume前闭合
  actionable/restart与blocked/ignored`EIO`，或winsize无法抑制same-value重复signal；
- BusyBox ash只能以`job control turned off`、无条件`TIOCSPGRP`、匿名console或专用kernel后门运行；
- 任一Active data-plane contract、Stage 3 relation matrix、RV64 build/KUnit/auto/vi回归失败，wrapper不能正常关机，
  或review仍有Apollyon/Keter/Euclid；
- 关闭目标需要把orphaned-pgrp、disassociation signals、`TOSTOP`、PTY、hangup或procfs TTY字段静默纳入本stage。

`TTY-JOBCTL-CUTOVER`只在全部source/lock/bypass audit、RV64 build/KUnit/auto ash+signal matrix、既有vi回归、用户人工
ash checklist与final review闭合后执行。cutover作为一个docs/contract unit：

- 在`docs/src/contracts/tty/job-control.md`原子激活`TTY-REL-001`、`TTY-JOBCTL-001`、`TTY-LIFE-001`、
  `TTY-ABI-001`，同步TTY contract索引与全局导航；四个ID不得部分生效；
- 将`ANE-20260527-PROCESS-GROUP-SESSION-STAGE1`收窄为仍未实现的relation-disassociation signals、newly orphaned
  stopped process-group policy和其它明确residual；按实际serial TTY证据收窄或保持
  `ANE-20260604-IOCTL-LTP-STAGE1-GAPS`，不得声称运行过LTP或关闭PTY/devpts/ptmx缺口；
  `ANE-20260529-PROC-TGID-STAT-STAGE1`保持不变；
- 同步RFC/implementation/tracker、transaction/index、`rfcs.md`和当前双周devlog，把R1与Stage 4记为Closed，
  区分agent-run、user-run、Not Run与accepted limitations。

任一proof不足时四个ID整体保持Not Cut Over，Stage 4保持Active / Awaiting Evidence或按finding停止；五个Active
data-plane ID不回退，也不能被写成完整RFC closure。退出条件是四个ID原子Active、register residual诚实、所有临时
bridge disposition明确、RFC R1与transaction关闭。Stage 4之后没有自动进入的下一stage。

### 9.6 Resolved Write Set Manifest

允许实现文件：

- `anemone-kernel/src/device/tty/{discipline.rs,terminal.rs,mod.rs,file.rs,relation.rs}`：control request、
  worker guards-out effect、background read gate、winsize change effect与owner-local KUnit；
- `anemone-kernel/src/task/jobctl/{mod.rs,terminal.rs}`：现有TTY caller/process-group capability上的窄read decision、
  stable identity revalidation与kernel-origin terminal signal request；
- `anemone-kernel/src/task/sig/{mod.rs,terminal.rs}`：只把现有TTY mask/disposition分类收窄泛化到
  `SIGTTIN/SIGTTOU`，不修改pending/generation/delivery truth；
- `anemone-kernel/src/task/sig/delivery.rs`：仅修复现有Signal-owned、trap-return-local
  `RestartSyscall` capability跨`UserEntryOutcome::Recheck` / jobctl park丢失的问题。restart capability不得进入
  `ThreadGroup`、jobctl phase、Signal pending或任何持久字段；`SIGCONT` wake只触发重新arbitration，custom
  handler是否restart仍由live `SA_RESTART`决定，最终无handler的`Alive + Running` entry才提交no-handler restart；
- `anemone-apps/tty-test/src/main.rs`：追加定向signal/background/winsize matrix、自动ash smoke与人工ash launcher；
- `scripts/run-tty-test-rv64.sh`：复用现有QEMU route，增加auto ash host oracle与`jobctl` mode；
- `conf/rootfs/tty-acceptance.md`：补充Stage 4 RV64人工ash checklist。现有
  `conf/rootfs/tty-acceptance-rv64.toml`保持不变。

允许cutover/生命周期文档写回：

- `docs/src/contracts/tty/{index.md,data-plane.md,job-control.md}`、`docs/src/contracts.md`、`docs/src/SUMMARY.md`；
- `docs/src/rfcs/tty-subsystem/{index.md,invariants.md,implementation.md,tracking-issues.md}`、`docs/src/rfcs.md`；
- `docs/src/devlog/transactions/{2026-07-23-tty-subsystem.md,index.md}`、
  `docs/src/devlog/2026-07-20_to_2026-08-02.md`；
- `docs/src/register/current-limitations.md`只允许上述三个entry的Narrow/Split/Unchanged回写。

Validation-only inputs：Stage 3 commit/diff/review与`build/tty-stage3-rv64.log`、TTY data-plane/current task Signal/
process-group/jobctl/lifecycle/user-entry contracts、register、Linux 6.6.32 `tty_jobctrl.c`/`n_tty.c`/winsize路线、
BusyBox 1.33.1 source与只读RV64 artifact、现有rootfs/Justfile/xtask/wrapper owners及只读RV64测试盘master。
公共文档不记录个人staging或artifact路径。

明确不允许：`anemone-abi/**`、`anemone-rs/**`、`anemone-kernel/src/fs/**`、`device/char/**`、TTY port/endpoint/
driver/console/IRQ、`task/topology/**`、`task/jobctl/{group.rs,report.rs,user_entry.rs}`、
`task/sig/{generation.rs,pending.rs}`以及超出上述短命restart handoff的`delivery.rs`变化、task lifecycle/exit、
scheduler/wait/Event、Kconfig、rootfs TOML、
pretest/user-test/LTP、architecture、external source/artifact/master。越过这些边界必须先扩展；owner、ABI、
visible semantics、cutover unit或acceptance boundary变化必须进入RFC review / Target Renegotiation Gate。

## 10. Stage 0 Ready：live interface、oracle 与 route resolution

阶段成熟度：

- `Ready`。本节已完整解析，但第2节实施入口与用户授权尚未满足，因此不是`Active`。

前置条件：

- 第2节全部完成；transaction路径与本节manifest一致。
- live branch、HEAD、dirty state与register重新核对；validation-only文件没有被其它并行工作替换。
- Stage 0执行者只做source/config/oracle审计，不修改kernel、apps、rootfs、tests或current contracts。

交付：

1. **VFS caller/open matrix。** 列出`FileIoCtx`、`IoctlCtx`、open provider、generic read/write/iomux与
   `get_current_task()`边界；确定TTY专用短命caller capability的注入点、允许保存的stable identity、
   禁止下沉的`Task`/fd-table/topology/Signal state，以及通用`CharDev`保持不变的证明。
2. **UART/transport matrix。** 列出NS16550A identity、boot-applied line config、现有raw major 234
   `CharDev`实现/注册的删除点、固定console + `TtyPort` projection、唯一RX consumer、IRQ drain、
   port-owned IRQ-safe TX route、probe rollback与consumer publication；比较preallocated storage与
   bounded fallible growth、per-port kthread与现有deferred facility、polling bounded batch与buffered TX，
   并按失败准则形成不含claim/lease/mode state的Stage 1单一路线建议。
3. **Endpoint/boot matrix。** 证明immutable port identity如何确定`ttyS<N>`、major 4/5 baseline、
   devfs单向publish、`/dev/console` owner、chosen stdout identity与boot fd安装的实际接线位置。
4. **Relation/topology matrix。** 列出Session/ProcessGroup/ThreadGroup live identity、membership、
   `setsid`、group move/removal、lifecycle与signal disposition/query路径；标出relation lookup、decision、
   generation revalidation和cleanup hook所需的窄owner API，不冻结最终Rust类型。
5. **Oracle matrix。** 以Linux 6.6.32 UAPI/TTY行为、Anemone现有jobctl test、目标BusyBox资料和wrapper
   能力为依据，列出data-plane、relation和job-control每个case的`tty-test`自动入口、QEMU/BusyBox
   自动smoke或用户交互入口、期望结果、provenance与当前缺口；定义用户提供BusyBox artifact的接入
   核对项、轻量rootfs/launcher路线、ash `TIOCSPGRP`三分支和vi raw/canonical/winsize调用。Stage 0
   不要求artifact已经到位，但必须把缺失artifact标为后续Gate P3和cutover的外部前置条件。
6. **模块边界结论。** 判断`ns16550a.rs`、`console.rs`、VFS ctx与task topology是否在继续增长前需要
   same-owner split-only checkpoint；只给出Stage 1/3的解析输入，不在Stage 0执行重构。

审计：

- 搜索全部`FileOps::{read,write,poll,ioctl}`构造、`FileIoCtx::new`、`IoctlCtx::new`、devfs open provider、
  status-flag与user-copy调用者，区分syscall current caller和kernel-internal调用。
- 搜索NS16550A probe、IRQ、raw `CharDev`实现/注册、console registration/output、minor allocation、
  firmware stdout selection与boot fd安装；建立固定console + `TtyPort`、raw registration删除点以及
  RX/TX/line/identity唯一owner表，确认没有claim/lease/mode state需求。
- 搜索Session/ProcessGroup lookup、create/move/remove、ThreadGroup detach/exit、process-group signal、
  disposition/mask与jobctl generation；建立relation acquire/mutate/cleanup所需handoff表。
- 对照可得且与目标artifact匹配的BusyBox `init.c`、`ash.c`、`vi.c`、`.config`和Linux termios/ioctl
  定义；artifact尚未到位或缺少匹配资料时使用已有目标版本作为带provenance的参考并显式记录缺口，
  不从主机libc header或不匹配的BusyBox defconfig猜测ABI。
- 核对register中的IRQ-off allocation、process-group/session、ioctl与proc-stat条目；不得把现有
  limitation当作实现正确性证明。

反馈假设：

- live VFS可以提供TTY专用短命caller capability而不扩大通用`CharDev`或让`Terminal`保存`Task`。
- live UART/allocator/deferred接口至少有一条路线满足bounded raw handoff、no-lost-work、IRQ约束、
  pre-publish rollback和port-owned IRQ-safe TX serialization，且固定console + `TtyPort`共存不需要
  claim、lease或mode state。
- 现有`KThreadHandle::wake()`是TTY使用的repository-owned窄notification carrier；其底层
  scheduler IRQ-off allocation风险继续由`ANE-20260622-IRQ-OFF-HEAP-ALLOCATION`的owner修复，
  不要求TTY新建workqueue、softirq或专用scheduler路径，也不把该共享实现债务当作Stage 0路线失败。
- live firmware/device identity可以形成不依赖probe完成顺序的稳定`ttyS<N>` mapping，并把chosen
  stdout映射为TTY可重验的endpoint identity。
- live topology/Signal接口可以用窄query/decision/revalidation闭合relation和`TIOCSPGRP`，不复制
  membership、foreground或jobctl truth。
- repository-owned rootfs入口可以形成不改变pretest默认流程的轻量TTY验收环境；目标BusyBox artifact
  到位后可以通过显式launcher建立真实session/controlling-terminal/foreground启动序列，交互证据
  不会被shell prompt或成功桩替代。

以下任一情况立即停止Stage 0并写入transaction：

- caller handoff只能通过所有FileOps保存完整`Task`、fd table、topology guard或Signal state实现；
- TTY-owning RX storage/IRQ路径本身只能进入不可控allocator/OOM/复杂drop，或只能依赖周期poll、
  TTY-private deferred infrastructure保证进展；调用现有`KThreadHandle::wake()`本身不触发此项，但
  不得以TTY局部patch掩盖其register中已记录的scheduler/wait-core风险；
- port identity只能依赖并发probe偶然顺序，chosen stdout无法映射到同一immutable endpoint；
- relation正确性要求Session与Terminal各保存一份mutable binding/foreground，或cleanup无唯一owner；
- 固定console + `TtyPort`共存只能通过runtime claim、lease、mode state或raw `CharDev`并行消费RX实现；
- BusyBox核心包络缺少可构造oracle，或其真实调用要求改变当前ABI/acceptance boundary；
- 实现只能通过owner、cutover unit、target guarantee或accepted limitation变化继续。

前四类若只是实现路线不足，Stage 0保持未关闭并形成带证据的route选项；后三类进入
`tracking-issues.md`与RFC review / `Target Renegotiation Gate`。不得在Stage 1用兼容桥绕过。

contract cutover：

- None。Stage 0只解析路线；全部`TTY-*`仍为Not Cut Over，现有contracts与register保持effective。

模块边界预检：

- `ns16550a.rs`当前混合line config、register I/O、console/CharDev projection、probe、bookkeeping与IRQ；
  Stage 0必须判断Stage 1是否先做same-owner split-only checkpoint，还是在新`device::tty`边界下只
  抽取driver-local capability即可。
- `console.rs`当前混合registry、selection、anonymous stdio与file ops；若`/dev/console` publication
  需要拆分，只允许same-owner、behavior-preserving、public API不扩大的结构维护。
- VFS ctx或task topology若需要public API/owner surface变化，不得包装成split-only；该变化进入相应
  Ready manifest与contract review。

scope envelope：

- 参与owner：VFS operation ctx、devfs、device number、console、NS16550A physical driver、TTY target、
  task topology、Signal/jobctl和validation harness；Stage 0对它们全部只读。
- 保护全部`TTY-*`target、Preserve contract IDs、两个cutover unit和正文acceptance boundary。
- Stage 0不选择或提交最终Rust字段/trait，不创建generic infrastructure，不修改ABI或current contract。

Resolved Write Set Manifest：

允许写入：

- `docs/src/devlog/transactions/2026-07-23-tty-subsystem.md`：只追加Stage 0 preflight、evidence matrix、
  finding、review与closure事实。

Stage 0 -> Stage 1 Resolution Gate另行允许写入：

- `docs/src/rfcs/tty-subsystem/implementation.md`：把Stage 1从Outline解析为Ready并冻结exact manifest；
- 上述transaction：记录resolution evidence、批准事实、生效点和实现计划链接。
- `docs/src/rfcs/tty-subsystem/index.md`、`docs/src/rfcs.md`与当前双周devlog：只同步Stage 1
  `Ready / Not Started`和下一步授权边界，不修改target、contract impact或历史Stage 0证据。

validation-only输入：

- `anemone-kernel/src/fs/{file.rs,api/ioctl.rs,api/openat.rs,api/read_write/,api/iomux/}`；
- `anemone-kernel/src/device/{char/,console.rs,devnum.rs}`；
- `anemone-kernel/src/fs/devfs/`、`anemone-kernel/src/driver/serial/ns16550a.rs`、
  `anemone-kernel/src/device/discovery/{fwnode.rs,open_firmware/chosen.rs}`、`anemone-kernel/src/main.rs`；
- `anemone-kernel/src/task/{topology/,jobctl/,sig/,files.rs,kthread/}`与现有Event/wait/deferred owner；
- `anemone-rs/`现有Linux UAPI、`anemone-apps/{jobctl-test,user-test}/`、app manifests、rootfs configs、
  xtask rootfs owner和repository wrappers；
- 本RFC、current contracts、register、Linux 6.6.32、用户提供的BusyBox artifact及可得的匹配
  source/config/provenance；artifact在Stage 0尚未到位时只读审计其接入contract和已有目标资料。

不允许触碰：

- kernel、apps、rootfs、test profile、build config、current contracts、register与RFC target文本；
- alpha worktree、共享的私人测试/参考资源和任何测试盘 master。

若审计本身需要代码instrumentation、test app或background evidence packet，Stage 0先停止并申请manifest
扩展；批准前不得先写。长证据包只有确有必要时才使用具体命名的`backgrounds/<topic>-probe-YYYYMMDD.md`，
不能新建generic `probe.md`/`feedback.md`。

可观测性：

- transaction中的每个矩阵项至少记录live symbol/path、owner、consumer、failure signal、候选路线、
  推荐路线和未决项；结论必须可由source/config重复核对。
- 不新增runtime log、counter或assertion；Stage 0若需要它们，说明只读证据不足并触发扩展/后续probe。

验证：

- `git diff --check`；
- `mdbook build docs`（`mdbook`可用时）；
- 对Stage 0 transaction、RFC links、contract IDs、register anchors与source paths做定向link/source audit；
- 确认kernel/apps/rootfs/test profile没有Stage 0 diff。本阶段不运行build、QEMU、BusyBox交互或LTP。

退出条件：

- 六类交付均形成可review矩阵，所有推荐路线都有live source/config依据与明确失败信号；
- owner、ABI、cutover unit和acceptance boundary未变化，或变化需求已经停止并进入RFC review；
- reviewer确认没有把实现偏好提升为target，也没有用generic infrastructure掩盖owner boundary；
- transaction追加Stage 0 Closed结论，并明确Stage 1 resolution所需输入。Stage 1是否已经Ready不是
  Stage 0关闭条件。

## 11. Stage 0 -> Stage 1 Implementation Resolution Gate（Completed 2026-07-23）

前置条件：

- Stage 0已经按自己的review、验证与退出条件独立关闭。

只读preflight：

- 读取Stage 0 evidence matrix、live source、当前diff/dirty state、review findings、register、RFC target、
  current contracts和仍有效的transport/identity/oracle结论。
- 核对Stage 1目的、依赖、受保护边界与target可达性；特别复查IRQ allocation issue、RX consumer
  publication、console/TTY TX recursion与endpoint rollback。

解析输出：

- 冻结单一路线的`TtyPort`最小capability边界、raw `CharDev`删除、固定console + `TtyPort` projection、
  唯一RX consumer、raw storage、deferred carrier、port-owned IRQ-safe TX serialization、immutable identity
  与pre-publish rollback；具体命名保持owner-local且不预建claim/lease/mode或follow-up surface。
- 把Stage 1展开为完整Ready阶段：精确交付/probe、审计、可观测counter/assertion、验证、停止条件、
  bridge删除点、contract cutover(None)和逐文件`Resolved Write Set Manifest`。
- 根据Stage 0的transport与module-boundary证据，明确Stage 1是否需要多个checkpoint；需要时冻结完整
  checkpoint map、每个checkpoint对stage manifest的归属、独立review/validation/stop边界与bridge退出点。
  若这些信息仍无法闭合，先调整Stage 1 future Outline，不得只把首个checkpoint标为Ready。
- manifest必须区分kernel写入、计划新建module、kconfig/KUnit、transaction write-back、validation-only
  inputs、不应触碰边界和integrator/reviewer责任。
- 若只改变future stage范围/顺序/验证安排，更新本文并在transaction记录；若改变target/owner/ABI/
  contract/cutover/acceptance boundary，停止并进入RFC review。

授权边界：

- 本轮preflight确认Stage 0 evidence与live source未漂移，R0 target/owner/ABI/contract/cutover/acceptance
  boundary无需更改。第6节已经冻结单一路线、三个checkpoint与exact manifest，Stage 1达到`Ready`。
- 用户只授权完成本resolution gate；仍需另行用户授权Stage 1或Checkpoint 1，不自动开始代码实现。

`1 -> 2` gate的完成记录见第12节，`2 -> 3` gate见第12.1节，`3 -> 4` gate见第12.2节。每个gate
都额外读取前一Stage实际diff、runtime evidence、bridge状态、module pressure与cutover结果，只解析下一个完整Stage
且不自动激活。

## 12. Stage 1 -> Stage 2 Implementation Resolution Gate（Completed 2026-07-23）

前置与只读preflight：

- Stage 1三个checkpoint已经独立关闭。gate读取`c25cc816` production tree、Stage 1实际diff/review、RV64
  KUnit/QEMU/RX burst、LA64 Not Run处置、bridge状态、R0 target、current contracts、register和clean worktree；
  没有把历史Outline或Stage 0候选路线当成live接口。
- 审计live `TtyPort`/attachment/kthread strong-reference shape、NS16550A applied-line/TX能力、FileOps与
  `IoctlCtx`、iomux `PollRequest`/Latch、devfs单向publish、console selection/anonymous stdio、post-Late boot
  ordering、app/rootfs/QEMU owner以及Linux 6.6.32 asm-generic UAPI。
- 用户提供的初赛盘mount只作只读artifact evidence。gate核对RV64/LA64 glibc static BusyBox的ELF/hash，
  并用可用的RV64 qemu-user确认1.33.1及`ash/stty/vi`；这些local路径不进入canonical接口。

解析决定：

- 采用Terminal-owned bounded output queue，不把Stage 1同步partial `submit_tx()`直接暴露为用户write truth；
  一个用户byte的完整transform token入队才算progress，echo复用同一queue，endpoint worker统一推进RX/TX/drain。
- 保留一个per-endpoint worker和predicate-driven wake；driver attachment长期持handle，open provider只持
  wake source的Weak引用，opened file可持窄strong capability；Terminal/endpoint/port不持worker handle。
  若实现证明仍有strong cycle，命中Checkpoint 1停止条件，不另建generic workqueue。
- termios使用asm-generic共享ABI；unsupported字段只允许unchanged，实际改变返回`EINVAL`。Stage 2 relationless
  `ISIG`真实执行discipline consume/flush/echo，但no foreground relation不产生Signal；winsize同理提交truth而
  延后foreground `SIGWINCH` effect。这保持R0 target和Stage 3/4 owner边界，不是success stub。
- `TtyPortId`全量排序决定`ttyS<N>`；console唯一提交selected-terminal identity，TTY只重验；所有Late完成后
  由boot coordinator显式finalize/publish并安装real stdio，避免依赖initcall/probe完成顺序。
- Stage 2解析为discipline/worker、FileOps/UAPI、publication/boot、userspace/cutover四个checkpoint；第7节
  冻结完整路线、逐checkpoint floor、双架构matrix、BusyBox staging、atomic contract cutover和exact manifest。

Target / contract / revision结论：

- live evidence不要求改变R0 owner、ABI包络、visible semantics、accepted limitation、两个cutover unit或最终
  acceptance；本轮只是保持target的实现解析，不修改`invariants.md`、tracking issue或RFC修订号。
- 全部`TTY-*`继续Not Cut Over。Stage 2达到`Ready / Not Started`，仍需新的明确授权才能激活Checkpoint 1；
  resolution gate不运行kernel build、QEMU、LTP或BusyBox guest test，也不自动进入Stage 2。

R1于C3关闭后的Target Renegotiation Gate取代本节R0的双架构C4 proof安排：Stage 2功能路线、case matrix与
cutover unit不变；除C4 acceptance文件集合按RV64收窄外，其余source manifest不变，runtime floor只要求RV64，
LA64明确Not Run。
该修订不改写上述R0 resolution的历史输入或当时结论，当前执行以第7节R1正文为准。

## 12.1 Stage 2 -> Stage 3 Implementation Resolution Gate（Completed 2026-07-23）

前置与只读preflight：

- Stage 2四个checkpoint已经Closed，`4f15e23d`原子完成`TTY-DATA-CUTOVER`；gate读取Stage 2实际diff、
  240项KUnit与RV64自动/人工证据、0 Apollyon / 0 Keter / 0 Euclid review、R1 target、五个Active TTY
  data-plane IDs、全部Preserve contracts、register和clean worktree。LA64按R1明确Not Run。
- live审计覆盖TTY endpoint/FileOps/boot publication、`FileIoCtx`/`IoctlCtx`/`DevfsNodeOps`、
  Session/ProcessGroup/ThreadGroup stable object、`setsid`/group move/removal、Signal mask/disposition/generation、
  `kernel_exit_group`与last-member exit、asm-generic TTY UAPI，以及现有`tty-test`/rootfs/RV64 wrapper。
- Linux 6.6.32 `tty_jobctrl.c`只作为ABI/errno与effect ordering参考；不复制Linux的`signal_struct::tty`、
  tasklist/RCU或内部锁结构。

解析决定：

- generic VFS ctx无需扩展：caller-relative open与TTY ioctl都能在同步entry即时派生短命caller capability；
  `fs/**`、devfs和CharDev因而保持只读。
- relation采用TTY-owned单registry、stable task-side capability与checked generation；runtime容量由boot endpoint
  数量预留，不增加Kconfig常量、第二索引或Session/Terminal mutable cache。
- Stage 3一次完成`/dev/tty`、五个ioctl、完整非orphan `TIOCSPGRP`三分支和detach/exit cleanup。只实现
  “结构路径、Stage 4再补真正decision”会制造可见临时ABI，因此本stage不拆checkpoint；Stage 4只增加
  terminal-generated signals/background read、ash集成与最终cutover。
- lifecycle使用task owner的opaque session-leader identity做一个直接、guards-out、幂等handoff，不建立generic
  callback/observer framework；所有lookup/mutation仍做live lifecycle revalidation，以关闭exit与late acquire交错。
- userspace复用现有`tty-test` auto mode、rootfs与RV64 wrapper，只追加relation matrix和最小anemone-rs wrapper；
  不增加app、manifest、launcher、wrapper mode或个人path。Stage 3验证只要求RV64。

Target / contract / authorization结论：

- 解析保持R1 target、owner、ABI包络、visible semantics、accepted limitations与两个cutover unit，不产生R2，
  不修改invariants、tracking issues、current contracts或register。`TTY-JOBCTL-CUTOVER`仍Not Cut Over。
- 第8节已经冻结单一路线、ABI matrix、review/validation、stop/exit与exact manifest，Stage 3达到
  `Ready / Not Started`。本gate只修改canonical docs并同步transaction/navigation；没有运行kernel/app build、
  KUnit、QEMU、BusyBox、LTP或hardware test，也不自动激活Stage 3。

## 12.2 Stage 3 -> Stage 4 Implementation Resolution Gate（Completed 2026-07-24）

前置与只读preflight：

- Stage 3已经在`5bf8024a`关闭；gate读取其实际diff、0 Apollyon / 0 Keter / 0 Euclid终审、239项KUnit、
  `TTYTEST:SUMMARY:PASS:34`与RV64 wrapper evidence，并核对R1 target、五个Active data-plane IDs、四个
  Not Cut Over IDs、全部Preserve contracts与register。LA64按R1继续Not Run。
- live source审计覆盖discipline signal-control结果、Terminal flush/echo/winsize与diagnostic bridge、TTY worker、
  FileOps read/ioctl、relation snapshot/generation、Stage 3 caller/process-group capability、Signal mask/disposition和
  process-group generation、现有`tty-test`、rootfs、RV64 wrapper及BusyBox ash/vi调用序列。
- Linux 6.6.32 `n_tty.c`、`tty_jobctrl.c`与winsize route只用于确认control-char mapping、background read的
  `SIGTTIN`/restart/`EIO`边界和changed-only`SIGWINCH`行为；不复制Linux tasklist/RCU/tty内部对象。

解析决定：

- Stage 4保持一个不拆checkpoint的最终stage。control char、background read与winsize都能复用Stage 3
  relation snapshot和现有ProcessGroup Signal入口；实现、自动/人工proof与四ID原子cutover没有第二个独立owner
  或可单独生效的中间contract，额外stage/checkpoint只会复制lifecycle。
- control char采用Terminal guard内形成无分配request、worker guards-out立即执行的直达路线，不增加persistent
  effect queue、generic notifier或第二work source；winsize setter只返回changed bit，effect同样guards-out。
- background read在现有TTY FileOps同步entry内构造caller并在每次消费前重验，不扩展`FileIoCtx`。Signal只把
  既有`SIGTTOU`分类窄化泛化到`SIGTTIN/SIGTTOU`；task capability提供受限terminal signal request，不改
  pending/generation/delivery、topology membership或ThreadGroup jobctl truth。
- userspace复用单一`tty-test`、`tty-acceptance-rv64` rootfs与wrapper；只给wrapper增加auto ash oracle和一个
  `jobctl`人工mode，不增加app、rootfs TOML、通用launcher或anemone-rs/ABI wrapper。验证仅RV64。
- final cutover建立一个TTY job-control contract surface并原子激活四个ID；register只按实际证据收窄，proc stat
  条目保持不变。人工证据缺失时Stage 4保持Active / Awaiting User Evidence，不拆新验收阶段。

Target / contract / authorization结论：

- 解析保持R1 target、owner、ABI、visible semantics、accepted limitations、RV64-only acceptance与
  `TTY-JOBCTL-CUTOVER`组成，不产生R2，不提前修改current contracts或register。
- 第9节已经冻结单一路线、signal/access matrix、review/validation、stop/exit/cutover与exact manifest，Stage 4
  达到`Ready / Not Started / Unauthorized`。本gate只修改canonical docs并同步transaction/navigation；没有修改
  kernel/app/wrapper实现，也没有运行build、KUnit、QEMU、BusyBox、LTP、LA64或hardware test，不自动激活Stage 4。

## 12.3 Stage 4 activation与Signal restart handoff扩展（Active 2026-07-24）

用户明确授权完成Stage 4后，candidate在原冻结manifest内实现terminal control effect、background read access、
changed-only winsize effect及自动/人工ash harness。首次完整RV64自动matrix通过239项KUnit、Stage 3 relation回归、
三种control character、blocked/ignored background read、winsize与detach case；actionable `SIGTTIN`被
`wait4(WUNTRACED)`观察为Stopped且input未在background消费，但`SIGCONT`后child从原read收到`EINTR`并退出1。

source audit确认TTY read已经在发送process-group `SIGTTIN`后返回`RestartSyscall::Idempotent`；既有
`arbitrate_user_entry()`却在首轮`handle_signals()`前对restart option执行`take()`，default-stop提交jobctl park后
没有消费该capability，但outer arbitration也无法在resume后的下一轮继续持有它。问题属于已有Signal/user-entry
handoff，不属于TTY relation或ThreadGroup job-control truth；若在default-stop分支无条件恢复syscall，反而会绕过
后续custom `SIGCONT` handler的live `SA_RESTART`决定。

本轮因此按第9.5节停止并上报。用户批准把`anemone-kernel/src/task/sig/delivery.rs`加入Stage 4 manifest，且只允许：

- restart capability继续作为当前trap-return arbitration的短命局部值，跨jobctl park / `Recheck`保留；
- `ThreadGroup`仍唯一拥有phase、exposure与report，`SIGCONT` generation/wake不持有也不提交restart truth；
- custom handler以live `SA_RESTART`消费或取消capability，无handler路径只在最终`Alive + Running` admission前恢复；
- 不修改Signal pending/generation、jobctl group/report/user-entry、architecture、public API或持久对象形状。

该扩展保持R1 target、owner、ABI、visible semantics、acceptance与`TTY-JOBCTL-CUTOVER`组成，不产生R2或current
contract变化。验证必须除Stage 4原floor外覆盖unix-jobctl focused/KUnit与source audit，证明stop/continue/report、
ordinary wait result、stale conditional stop、temporary-mask和user-entry gate均未被restart handoff反向驱动。

修复后同一candidate完成agent-run floor：repository wrapper中的239项KUnit全部通过，TTY自动matrix为
`TTYTEST:SUMMARY:PASS:45`，BusyBox vi与ash oracle均通过并正常关机；独立pretest wrapper中的19项
`jobctl-test` focused case全部通过，覆盖stop/continue、wait4/waitid/SIGCHLD、temporary mask、multi-member、
process-group broadcast、exec/dethread与SIGKILL。该pretest测试盘没有对应LTP executable，signal/wait profile为
`attempted=0`，因此不构成LTP证据。source/lock/bypass audit确认restart capability没有进入ThreadGroup、jobctl
phase、Signal pending或其它持久truth，但对抗review发现`check_read_access()`曾把`TtyCaller::current()`的所有
失败都当作kernel-internal caller而fail-open；user ThreadGroup在lifecycle/topology瞬时重验失败时可能因此绕过
`SIGTTIN/EIO` gate。修复将现有窄接口拆为`current_user_or_kernel()`：只有KThread返回`Ok(None)`，user caller的
任何构造/重验失败继续向read返回错误，不再授权input消费。该Keter在同一owner与原manifest内关闭，不改变
target/ABI/cutover。修复后的完整auto/focused runtime已经重跑通过，对抗review确认代码层最终为
0 Apollyon / 0 Keter / 0 Euclid。

candidate证据为`dev/drc/omega@3249034c`加当前dirty Stage 4 diff、RV64
`qemu-virt-rv64-pretest`、kernel SHA-256
`b157b6f36c3a413283d86a827c60f777554806a21da5f5b98eb921ec7df7554e`、BusyBox SHA-256
`fd9cb9dc66ba740dc94b055b564de0597453adfceef9be158b3774ca58b95241`，自动日志为
`build/tty-stage4-rv64.log`，focused日志为`build/tty-stage4-jobctl-regression-rv64.log`。Stage 4现在为
**Active / Awaiting User Evidence**；用户仍须在同一candidate与输入上完成第9.4节`--mode jobctl` checklist，
在此之前不得执行`TTY-JOBCTL-CUTOVER`、修改current contracts/register或提交Stage 4 checkpoint。

用户首次执行人工checklist时，尚未输入完第一条命令，guest即在约3秒后关机；日志中的
`TTYTEST:FAIL:manual-ash-jobctl:5`不是TTY/job-control acceptance failure。source audit确认人工ash launcher错误
复用了自动oracle的`300 * 10ms` bounded wait，deadline到期后主动kill/reap child并进入汇总关机。修复保持自动路径
的有界deadline，只让人工路径阻塞等待用户显式退出ash，并继续校验exact child与异常cleanup；该修改位于既有
`tty-test` Stage 4 manifest内，不改变target、owner、ABI或acceptance boundary。该次运行不得作为人工证据，修复后
仍须重新执行完整checklist；在复测与自动回归通过前Stage 4保持**Active / Awaiting User Evidence**且四个ID整体
Not Cut Over。

修复后的首次auto回归还暴露host oracle的输入分块竞态：ash marker与prompt落在同一次read时，原`if/elif`
状态推进只识别marker，等待新输出才会识别已经缓存的prompt，最终先触发guest自动deadline。wrapper现允许同一
buffer连续完成marker到prompt的推进；自动deadline、ash命令序列和guest语义均不改变。该失败运行同样不是cutover
evidence。修正后的完整auto已经重新通过239项KUnit、TTY `45/45`、BusyBox vi/ash oracle与host byte checks，
kernel SHA-256仍为`b157b6f36c3a413283d86a827c60f777554806a21da5f5b98eb921ec7df7554e`并正常关机；当前只剩用户
重新执行人工checklist，Stage 4继续**Active / Awaiting User Evidence**。

## 12.4 Stage 4用户证据、`TTY-JOBCTL-CUTOVER`与关闭（Closed 2026-07-24）

用户在修正后的同一candidate上完成冻结的人工ash job-control checklist。日志
`build/tty-stage4-rv64-jobctl.log`记录base `3249034c`、platform `qemu-virt-rv64-pretest`、BusyBox SHA-256
`fd9cb9dc66ba740dc94b055b564de0597453adfceef9be158b3774ca58b95241`与kernel SHA-256
`b157b6f36c3a413283d86a827c60f777554806a21da5f5b98eb921ec7df7554e`；239项KUnit全部通过。交互过程真实覆盖
Ctrl-C、Ctrl-Z、`jobs`、`fg`、`bg`，background `cat`显示`Stopped (tty input)`，重新置于foreground后可读取输入并
由Ctrl-C终止，最后由用户显式`exit`结束ash。日志给出`TTYTEST:PASS:manual-ash-jobctl`、
`TTYTEST:SUMMARY:PASS:2`与host `TTY-HARNESS:PASS:jobctl`，且没有`job control turned off`。用户在checklist间
额外执行的`ls`、`busybox`、`clear`与`jobs`不改变冻结断言或验收结果。

自动日志`build/tty-stage4-rv64.log`仍证明239项KUnit、TTY `45/45`、BusyBox vi/ash oracle与host byte checks；
focused日志`build/tty-stage4-jobctl-regression-rv64.log`仍证明19项既有`jobctl-test`全部通过。最终source/lock/
bypass与contract review为0 Apollyon / 0 Keter / 0 Euclid。因此`TTY-JOBCTL-CUTOVER`作为单一unit原子激活
`TTY-REL-001`、`TTY-JOBCTL-001`、`TTY-LIFE-001`与`TTY-ABI-001`，current truth转移到
[TTY job-control contract](../../contracts/tty/job-control.md)；没有改变任何Preserve contract，也没有部分cutover。

register按实际证据处置：`ANE-20260527-PROCESS-GROUP-SESSION-STAGE1`与
`ANE-20260604-IOCTL-LTP-STAGE1-GAPS`均Narrowed，`ANE-20260529-PROC-TGID-STAT-STAGE1`保持Unchanged。
relation-disassociation `SIGHUP/SIGCONT`、newly orphaned stopped group、orphaned-pgrp、`TOSTOP`及其它
terminal-modifying operation，连同PTY/devpts/ptmx等仍为residual。LTP profile为`attempted=0`，LA64
compile/runtime与hardware均Not Run，不能由RV64证据外推。

Stage 4、RFC R1与transaction至此全部Closed。Stage 4之后没有下一stage，本次不进入任何后续gate。

## 13. Probe / Vertical Slice Gates

### Gate P1 — UART-to-Terminal transport

**Hypothesis:** 现有NS16550A和deferred设施可在不扩散hardware owner的前提下删除raw `CharDev`，固定
同时提供output-only console backend与`TtyPort`，形成有界、有序、可观测且无lost-work的TTY独占raw
handoff，并用port-owned IRQ-safe serialization统一console/TTY普通TX，而不需要claim/lease/mode state。

**Protected Goal / Invariant:** `TTY-PORT-001`、`TTY-OUTPUT-001`、IRQ hard boundary、no polling as
progress source和pre-publish rollback。

**Contract Impact:** 只验证candidate；Stage 1不cutover。

**Minimum Write Set:** 由Stage 0 -> 1 gate冻结到NS16550A driver-local surface、实际创建的
`device::tty`最小module、必要kconfig/KUnit和transaction；不触碰VFS caller、topology或jobctl。

**Non-goals:** generic workqueue/transport framework、raw serial/serdev frontend、runtime line reconfiguration、
claim/lease/personality state、hangup、devfs publish、PTY、周期polling fallback。

**Validation Floor:** source/lock audit、KUnit、repository build、IRQ/raw counter证据和最小QEMU RX burst
probe；精确命令由Stage 1 Ready冻结。

**Failure Signals:** raw `CharDev`仍注册、console消费RX、需要runtime claim/lease/mode switch、lost byte/order、
silent overflow、IRQ allocation/OOM side effect、recursive printk/TX、无界IRQ-off write、worker publication
race、必须poll才进展或rollback后仍可达。

**Write-back / Exit:** 结果进入transaction；路线保持target则升级为Stage 2输入并删除probe-only
surface，owner/ABI/lifecycle变化则回RFC review。

### Gate P2 — caller/relation decision and commit

**Hypothesis:** TTY FileOps可从同步caller构造窄capability，relation owner和topology/Signal owner可用
snapshot -> guards-out decision -> generation revalidation -> commit闭合`/dev/tty`与`TIOCSPGRP`。

**Protected Goal / Invariant:** `TTY-REL-001`、`TTY-JOBCTL-001`、`TTY-LIFE-001`与全部Preserve IDs，
包括`SIGNAL-PENDING-001/002`和`SIGNAL-ACTION-001/002`。

**Contract Impact:** Stage 3只验证candidate；不cutover。

**Minimum Write Set:** 第8.6节已经冻结到TTY relation/FileOps、task-side窄caller/topology/Signal decision、
lifecycle handoff、定向test和transaction；不修改generic VFS ctx、ThreadGroup jobctl truth或scheduler/wait owner。

**Non-goals:**完整Task持久引用、双向mutable cache、relation-disassociation signals、orphan policy、
hangup、`TOSTOP`完整matrix、PTY。

**Validation Floor:** KUnit/source lock audit、定向userspace acquisition/lookup/foreground/reuse/cleanup cases、
repository build与RV64 QEMU auto regression；精确命令见第8.4节，LA64明确Not Run。

**Failure Signals:** session外target、ID reuse复活、detach后仍discoverable、mutation先于decision、锁反转、
需要第二份membership/foreground truth或无法取得current caller。

**Write-back / Exit:** 保持target则形成Stage 4输入；owner/ABI/cleanup/cutover变化立即回RFC review。

### Gate P3 — BusyBox ash/vi acceptance

**Hypothesis:** target列出的最小TTY ABI足以让最终BusyBox ash和vi走真实data/relation/jobctl路径，
不依赖success stub或global fallback。

**Protected Goal / Invariant:** `TTY-ABI-001`、`TTY-LOCAL-001/002/005/006`与两个cutover unit。

**Contract Impact:** Stage 2可执行`TTY-DATA-CUTOVER`；只有Stage 4完整matrix通过后执行
`TTY-JOBCTL-CUTOVER`。

**Minimum Write Set:** 由相应Ready stage冻结到TTY实现、`tty-test`、轻量TTY rootfs/launcher、必要的
repository wrapper、transaction、cutover contracts/register；用户提供的BusyBox原件、对应外部
source/config/provenance和测试盘master始终只读，artifact不得提交到Anemone仓库或被原地修改。

**Non-goals:** GNU Vim、PTY、runtime line reconfiguration、hangup、完整Linux corner matrix。

**Validation Floor:** `tty-test`自动matrix先覆盖可判定的data/relation/job-control语义；轻量rootfs中的
BusyBox再覆盖vi启动/编辑/保存/退出、ash无job-control降级、Ctrl-C、两轮Ctrl-Z/jobs/fg/bg foreground
交接、ordinary background read、relation撤销后`/dev/tty`不可取得，以及`TIOCSPGRP`三条定向路径。
agent负责build、自动matrix、QEMU自动smoke与日志分析；用户按冻结checklist执行Stage 2 vi/data-plane和
Stage 4 ash/job-control人工验收并回传artifact identity、逐项结果与串口日志。首版不把detach/exit
`SIGHUP`/`SIGCONT`列为通过条件；R1只要求RV64 build/runtime与人工验收，LA64明确Not Run；证据区分
source、KUnit、build、agent-run QEMU、user-run与未运行。

**Failure Signals:** BusyBox artifact架构/runtime/applet不满足验收输入、launcher没有建立真实session/
controlling-terminal/foreground序列、shell prompt被当作closure、fake ioctl、raw mode仍signalize、
foreground reclaim失败、session外signal、test-only fallback、人工结果替代自动matrix或延期项被伪装成通过。

**Write-back / Exit:** 分层证据进入transaction，明确记录BusyBox artifact identity、轻量rootfs/launcher、
agent-run与user-run结果；人工发现的稳定语义缺陷尽可能先转为自动回归。只在对应unit全部proof满足时
cutover，否则保持Not Cut Over并按target范围分类open issue、limitation或Target Renegotiation。

## 14. 旁路审计清单

- `open_console_stdin`、`open_console_stdout`、anonymous console inode/FileOps和boot fd安装点；
- NS16550A `CharDev`实现与raw major 234 registration、任何claim/lease/mode state、console RX消费、
  IRQ byte discard、每IRQ printk与绕过port-owned serialization的TX；
- 所有`/dev/tty*`、major 4/5、foreground PGID、SID/PGID numeric cache与device identity生成点；
- 所有TTY候选polling timer/watchdog、worker pending flag、wake payload与queue shadow；
- 所有termios/ioctl success stub、unknown command default、compat bits silent path和unsupported log；
- 所有terminal signal producer、direct stopped-bit/report mutation、current-task/last-reader/global-PGID fallback；
- 所有last-close、session exit、group removal、driver failure与node removal路径；
- 所有hard-IRQ/noirq allocation、format/log、complex drop、Event/Signal/task-topology call和sleepable lock。

允许保留旁路必须有独立owner、不会消费同一RX truth，并在相应Ready stage写明可见行为和删除/保留
条件；否则不得越过对应cutover。

## 15. 可观测性清单

后续Ready stage必须按实际路线冻结：

- per-port RX bytes、overflow/drop、line-error、notification、drain batch/budget与no-lost-work assertion；
- endpoint identity/name/devnum、create/publish结果、rollback原因与duplicate rejection；
- termios unsupported/normalized/ignored字段的限频诊断，禁止每字节或每IRQ日志；
- input predicate/read/poll source一致性assertion、canonical boundary与flush计数；
- relation acquire/detach/generation mismatch/retry、foreground change、decision分类和signal target identity；
- bridge启用次数、raw endpoint结论与probe-only watchdog删除证据；
- cutover时每个contract ID、验证provenance、Not Cut Over项与register disposition。

纯诊断字段不得驱动state machine；一旦字段参与行为，必须按协议状态进入owner/不变量/review。

## 16. 全局停止边界

以下情况停止当前stage，不继续用局部patch追进度：

- 需要改变target owner、ABI、cutover unit、accepted limitation或完整验收包络；
- 出现第二份input/termios/relation/foreground/jobctl truth、lost wake/work、session外signal、half-published
  endpoint、unowned cleanup、hard-IRQ复杂工作、deadlock或内存安全风险；
- natural implementation需要越过Ready manifest。先提交扩展理由、拟新增文件/owner、contract/gate/
  验证影响，批准并更新本文/transaction后再继续；
- validation只能通过success stub、test-specific fallback、周期polling或缩小测试集合完成；
- source/runtime证据表明原target代价不可接受。记录cost evidence与partial-code disposition，进入
  `Target Renegotiation Gate`，未经review批准不得降低target或cutover。

如果剩余差异只属于已明确延期且ABI诚实的corner、future Outline内部类型选择或不影响owner/contract/
acceptance的代码风格，停止扩大本RFC；把有真实consumer/oracle的扩展留给follow-up revision/RFC。

调用repository-owned `KThreadHandle::wake()`属于本RFC允许的窄deferred notification，不要求TTY
穿透scheduler实现或为开放的IRQ-off allocation问题发明替代设施。TTY仍必须保证自身IRQ handler、
raw storage、counter与notification调用前后不新增allocation、复杂drop、普通日志或sleepable lock；
wait-core/scheduler对wake placement的后续修复由其现有owner与register流程闭合。

## 17. 实现期反馈与记录

2026-07-23 Stage 0 carrier审计确认现有`KThreadHandle::wake()`底层仍受register中的scheduler
IRQ-off allocation问题影响。用户按owner边界决定：TTY直接使用现有wake capability，不因此停摆或
发明workqueue/softirq/专用scheduler路径；该共享问题继续由wait-core/scheduler owner修复。本文已把
这项处置折回Stage 0反馈假设、停止条件和全局边界；它不改变R0 target、owner、ABI、cutover或
acceptance，不产生revision bump或Ready-manifest扩展。

- Execution Fact、checkpoint、review与验证只追加到transaction。
- Route Correction若保持target，只更新本文future Outline/Ready和transaction，不增加RFC修订。
- target/owner/ABI/visible semantics/acceptance变化先更新RFC review与tracking；接受后才形成新revision。
- current contract只在两个approved cutover gate原子更新；Draft RFC、probe与partial candidate不能生效。

## 18. 收口后验收工具维护

2026-07-24对公共TTY验收工具做一次target-preserving maintenance：repository wrapper不再用
SHA-256、BusyBox版本或host `qemu-riscv64`锁定某个本地artifact。wrapper只核对当前最小
rootfs真实需要的RV64 static ELF条件；guest launcher只核对验收case实际调用的
`ash`/`sleep`/`stty`/`vi` applet，再由原有data-plane、vi和ash matrix判定外部可见行为。

第3、7和9节的固定artifact identity文本仅记录R1当时的冻结计划与cutover输入；实际使用的
BusyBox 1.33.1、BusyBox/rootfs/kernel hash与同一candidate关系仍保留在transaction作为历史
provenance，不再成为live runner的兼容门槛。本维护不改变TTY target、owner、ABI、visible semantics、
current contract或R1 cutover结论，因此不产生R2，也不改写已完成stage与transaction证据。
