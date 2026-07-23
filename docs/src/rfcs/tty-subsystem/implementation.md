# TTY Subsystem 迁移实施计划

**状态：** Active / Stage 0 Closed / Stage 1 Closed / Stage 2 Closed / Stage 3 Outline
**最后更新：** 2026-07-23
**父 RFC：** [RFC-20260722-tty-subsystem](./index.md)
**目标与不变量：** [TTY Subsystem 目标与不变量](./invariants.md)
**当前契约：** [TTY data plane](../../contracts/tty/data-plane.md)中的五个`TTY-*` ID为Active；Preserve项及其链接见[Contract Impact](./invariants.md#contract-impact)。
**当前修订：** R1
**事务日志：** [2026-07-23 - TTY Subsystem](../../devlog/transactions/2026-07-23-tty-subsystem.md)
**Contract Cutover：** `TTY-DATA-CUTOVER` Effective；`TTY-JOBCTL-CUTOVER` Not Cut Over

本文把 TTY target 解析为可滚动实施的阶段、probe、验证和 cutover 边界，不重新定义
[`index.md`](./index.md) 与 [`invariants.md`](./invariants.md) 已经拥有的 target、owner、ABI
或 proof obligations。R1 已接受并由现有 active transaction 继续实施；Stage 0 获得单独授权后完成只读审计并关闭，
Stage 0 -> Stage 1 Resolution Gate 随后在新的明确授权下完成。Stage 1的三个checkpoint现已独立关闭；
Checkpoint 3采用获准的driver-local quiescent probe / Late activation路线完成production transport
candidate。Stage 1 -> Stage 2 Resolution Gate解析的四个checkpoint现已逐项完成，RV64自动matrix与用户人工
vi evidence达到floor，`TTY-DATA-CUTOVER`已经Effective并关闭Stage 2。Stage 3保持Outline；本轮没有执行
`2 -> 3` resolution gate或激活。

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
| Stage 3 | Outline | 建立 controlling relation、`/dev/tty`、caller/topology handoff、foreground ioctls 与 cleanup | None | 独立`2 -> 3` resolution gate；尚未执行或授权 |
| Stage 4 | Outline | 闭合 terminal signals、background access、ash job control、register 与 current contract | `TTY-JOBCTL-CUTOVER` | Stage 3 独立关闭后 |

阶段顺序保护“先闭合真实数据通路，再接 relation/jobctl”的验证路径，不冻结最终文件数量。
当前粒度只表示语义路线，不表示每个 Stage 应在一个执行单元内完成。Stage 1的三个checkpoint和Stage 2的
四个checkpoint已经分别由对应resolution gate冻结；Stage 3与Stage 4仍只保留caller handoff /
relation-ioctl / lifecycle cleanup，以及terminal signal-access / ash-vi acceptance / docs cutover等候选切面，
这些future Outline scope envelope不冻结编号、精确顺序、文件或授权。相应resolution gate可在保持target和
两个cutover unit不变时收窄、合并或改画future切面；不得为了缩短checkpoint发布半初始化endpoint、提前
执行cutover，或把跨checkpoint bridge留成长期抽象。

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

## 8. Stage 3 Outline：controlling relation 与 caller/topology vertical slice

概括目的：

- 建立 session-terminal 单一 relation registry 与 stable identity/generation revalidation；成功
  `TIOCSCTTY(arg=0)` 同时建立 relation 并把 foreground PGID 初始化为 caller 当前 process group。
- 提供 `/dev/tty` caller-relative open、`TIOCGSID`、`TIOCGPGRP`、`TIOCSPGRP` 与 session-leader
  `TIOCNOTTY` 的结构性路径；`arg=1` steal 和 non-leader局部 detach按 target稳定拒绝。
- 在同步 VFS entry构造短命 caller capability/snapshot，建立 topology/Signal窄 decision API和
  relation-generation commit revalidation；完整 `Task`、topology guard与Signal state不进入Terminal。
- 接入 session leader exit/detach cleanup：先撤销 `/dev/tty` 与foreground check可发现性，再
  guards-out 完成必要wake/drop；首版不生成relation-disassociation `SIGHUP`/`SIGCONT`。foreground
  group消失只使selector失效，不拆relation或endpoint。

前置依赖：

- Stage 2 Closed且`TTY-DATA-CUTOVER`结果明确；真实Terminal FileOps、boot stdio和endpoint identity
  已稳定，不能再用anonymous console验证relation。

受保护边界：

- `TTY-REL-001`、`TTY-JOBCTL-001`、`TTY-LIFE-001`以及所有Preserve contract IDs，特别是
  `SIGNAL-PENDING-001/002`与`SIGNAL-ACTION-001/002`。
- `Session`/`ProcessGroup`继续唯一拥有membership；TTY relation registry唯一拥有binding与foreground。
- relation commit必须在topology decision后重验generation；numeric SID/PGID reuse不得复活旧binding。
- last close不拆relation；session exit/detach不销毁Terminal、`/dev/ttyS<N>`或`/dev/console`。
- orphan transition/effect、hardware hangup、backend fatal、`TOSTOP`与完整terminal-modifying matrix不进入本阶段。
- 本阶段只形成relation/caller vertical slice，不执行`TTY-JOBCTL-CUTOVER`。
- Stage 3 candidate可以定向运行验证，但不能独立宣称relation/job-control已经effective；尚未闭合的
  background/signal分支必须稳定拒绝或保持不可达，不能先无条件成功再等待Stage 4修正。

解析触发点：

- Stage 2 关闭后重新审计 `FileIoCtx`/`IoctlCtx`/open provider、`setsid`/topology/lifecycle、Signal
  disposition与process-group selector；以 live lock graph冻结caller capability、relation index、
  cleanup hook、retry/errno和逐文件 manifest。

预计范围（不是写入授权）：

- `device::tty` relation/file/ioctl modules、窄VFS operation ctx、task topology/session lifecycle、
  process-group/Signal decision API，以及定向 relation/caller userspace tests。

## 9. Stage 4 Outline：terminal job control、完整验收与 `TTY-JOBCTL-CUTOVER`

概括目的：

- 让 foreground `VINTR/VQUIT/VSUSP`、winsize `SIGWINCH`、ordinary background read `SIGTTIN`
  通过 relation snapshot、membership revalidation和现有process-group Signal/jobctl路径真实生效。
- 闭合 `TIOCSPGRP` 非orphan三分支：foreground allow、background + blocked/ignored `SIGTTOU`
  allow，以及background + actionable `SIGTTOU` signal-and-restart且mutation不提前提交。
- 以 BusyBox ash/vi和定向oracle覆盖controlling acquisition、`/dev/tty`、Ctrl-C/Ctrl-Z、
  `jobs/fg/bg`、background read、foreground reclaim和relation撤销cleanup；明确标注relation-disassociation
  `SIGHUP`/`SIGCONT`与其它延期corner。
- 扩展`tty-test`以自动覆盖relation、foreground/background decision和可判定的terminal-signal路径；
  对控制字符、ash交互状态与vi显示只保留冻结的自动smoke和用户人工checklist，不让手测承担全部回归。
- 原子完成`TTY-JOBCTL-CUTOVER`，回写current contracts、transaction、register/current limitations、
  RFC/tracker/devlog并关闭或明确Not Cut Over。

前置依赖：

- Stage 3 Closed；relation/caller/topology API、cleanup顺序、retry/revalidation与errno已经过定向验证。
- `TTY-DATA-CUTOVER`已Effective；若为Not Cut Over，Stage 4不得以job-control代码绕过数据面缺口。

受保护边界：

- `TTY-REL-001`、`TTY-JOBCTL-001`、`TTY-LIFE-001`、`TTY-ABI-001`、
  `TTY-LOCAL-001/002/005/006`和全部Preserve contract IDs，包括`SIGNAL-PENDING-001/002`与
  `SIGNAL-ACTION-001/002`。
- TTY不得写ThreadGroup stop/report/user-entry truth；Signal result不得反向决定foreground selector。
- ash源码和定向路径已经为`TIOCSPGRP`非orphan三分支提供oracle，不能把它们降为limitation。
- `TOSTOP`、其它terminal-modifying `SIGTTOU` matrix、orphaned-pgrp errno/effect、runtime line change、
  relation-disassociation `SIGHUP`/`SIGCONT`、hardware hangup/backend fatal与PTY继续在target之外；
  unsupported必须ABI诚实。

解析触发点：

- Stage 3 关闭后读取实际relation/caller diff、jobctl current contract、`tty-test`结果、用户提供的
  BusyBox artifact identity/调用序列、轻量rootfs/launcher证据、runtime evidence与register状态，冻结
  signal/access矩阵、自动smoke、用户人工checklist、cutover docs manifest与review责任。

Contract cutover：

- 原子激活`TTY-REL-001`、`TTY-JOBCTL-001`、`TTY-LIFE-001`、`TTY-ABI-001`；任一proof不足时
  整个unit保持Not Cut Over，已Effective的`TTY-DATA-*`不因此被伪装成完整RFC closure。
- 按实际证据 Narrow/Split/Unchanged `ANE-20260527-PROCESS-GROUP-SESSION-STAGE1`与
  `ANE-20260604-IOCTL-LTP-STAGE1-GAPS`；`ANE-20260529-PROC-TGID-STAT-STAGE1`保持不变，
  除非另一次target review明确扩展范围。process-group/session residual必须继续显式保留
  relation-disassociation `SIGHUP`/`SIGCONT`和newly orphaned stopped process-group policy。

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
- alpha worktree、共享`etc/final`/`etc/xref`资源和任何测试盘master。

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

`1 -> 2` gate的完成记录见第12节。后续`2 -> 3`与`3 -> 4` resolution gate遵循同一协议，并额外读取
前一Stage实际diff、runtime evidence、bridge状态、module pressure与cutover结果。每次只解析下一个Stage，
并按第4节为预期的多checkpoint Stage冻结完整checkpoint map；不提前解析更远Stage的checkpoint细节。

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

**Minimum Write Set:** 由`2 -> 3` gate冻结到TTY relation/FileOps、窄VFS ctx、topology/Signal decision、
lifecycle hook、定向test和transaction；不修改ThreadGroup jobctl truth或scheduler/wait owner。

**Non-goals:**完整Task持久引用、双向mutable cache、relation-disassociation signals、orphan policy、
hangup、`TOSTOP`完整matrix、PTY。

**Validation Floor:** KUnit/source lock audit、定向userspace acquisition/lookup/foreground/reuse/cleanup cases、
repository build与QEMU smoke；精确命令由Stage 3 Ready冻结。

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
