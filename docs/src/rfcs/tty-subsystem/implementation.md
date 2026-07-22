# TTY Subsystem 迁移实施计划

**状态：** Active / Stage 0 Closed
**最后更新：** 2026-07-23
**父 RFC：** [RFC-20260722-tty-subsystem](./index.md)
**目标与不变量：** [TTY Subsystem 目标与不变量](./invariants.md)
**当前契约：** Preserve 项及链接见 [Contract Impact](./invariants.md#contract-impact)；`TTY-*` 均未 cut over。
**当前修订：** R0
**事务日志：** [2026-07-23 - TTY Subsystem](../../devlog/transactions/2026-07-23-tty-subsystem.md)
**Contract Cutover：** prospective `TTY-DATA-CUTOVER` 与 `TTY-JOBCTL-CUTOVER`；当前均为 Not Cut Over

本文把 TTY target 解析为可滚动实施的阶段、probe、验证和 cutover 边界，不重新定义
[`index.md`](./index.md) 与 [`invariants.md`](./invariants.md) 已经拥有的 target、owner、ABI
或 proof obligations。R0 已接受并建立 transaction；Stage 0 获得本轮单独授权后完成只读审计并
关闭。Stage 1 仍为 `Outline`，没有执行
Stage 0 -> Stage 1 Resolution Gate，也没有 current contract 因本文生效。

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
| Stage 0 | Closed | 只读闭合 live interface、oracle、carrier 候选与模块边界 | None | 已关闭；Stage 1仍需独立resolution gate与授权 |
| Stage 1 | Outline | 建立 unpublished port/Terminal transport vertical slice，闭合 IRQ、RX、TX 与 pre-publish transaction | None | Stage 0 独立关闭后 |
| Stage 2 | Outline | 交付 line discipline、termios、read/write/poll、`/dev/ttyS<N>` 与 real boot stdio | `TTY-DATA-CUTOVER` | Stage 1 独立关闭后 |
| Stage 3 | Outline | 建立 controlling relation、`/dev/tty`、caller/topology handoff、foreground ioctls 与 cleanup | None | Stage 2 独立关闭后 |
| Stage 4 | Outline | 闭合 terminal signals、background access、ash job control、register 与 current contract | `TTY-JOBCTL-CUTOVER` | Stage 3 独立关闭后 |

阶段顺序保护“先闭合真实数据通路，再接 relation/jobctl”的验证路径，不冻结最终文件数量。
当前粒度只表示语义路线，不表示每个 Stage 应在一个执行单元内完成。Stage 1 是否需要多个 checkpoint
由 Stage 0 的 transport/module-boundary 证据决定；Stage 2、Stage 3 与 Stage 4 预期必须解析为多个
checkpoint。候选切面分别是 data-plane core / FileOps-UAPI / publish-boot-acceptance、caller handoff /
relation-ioctl / lifecycle cleanup，以及 terminal signal-access / ash-vi acceptance / docs cutover；这些只
是 scope envelope，不冻结编号、精确顺序、文件或授权。相应 resolution gate 可在保持 target 和两个
cutover unit 不变时收窄、合并或改画切面；不得为了缩短 checkpoint 发布半初始化 endpoint、提前执行
cutover，或把跨 checkpoint bridge 留成长期抽象。

## 6. Stage 1 Outline：unpublished transport vertical slice

概括目的：

- 在 UART physical owner 与 `device::tty` terminal owner 之间建立最小窄 capability；选择 raw RX
  storage、deferred carrier、TX serialization 与 boot-applied line snapshot 的实现路线。
- 让 NS16550A 从同一 physical state 固定构造 console backend 与 `TtyPort`，删除 raw `CharDev`
  implementation/registration；console 不参与 RX 或运行期线路配置，pre-publish `TtyPort` consumer
  binding 是 RX 的唯一去向，不建立 claim/lease/mode state。
- 建立完整但尚未 devfs-published 的 endpoint create transaction，验证 attach、IRQ publication、
  notification-plus-predicate、consumer drain 与 publish 前 rollback。
- 将 NS16550A RX 从“IRQ 中读取后丢弃”迁移为单 owner raw handoff；清除 RX IRQ 中递归 printk，
  并让 console/TTY 普通 TX 经过 port-owned IRQ-safe serialization；polling 路线使用有界 batch，
  或由有界 TX queue保持唯一 owner，不把任意长度用户 write 变成无界 IRQ-off 临界区。

前置依赖：

- Stage 0 Closed；Stage 0 -> Stage 1 Resolution Gate 已冻结完整 Stage 1 与 exact manifest。
- `ANE-20260622-IRQ-OFF-HEAP-ALLOCATION` 的约束已经进入 carrier/storage 选择，不以 noirq allocator
  或偶然 smoke 作为安全证明。

受保护边界：

- `TTY-PORT-001`、`TTY-OUTPUT-001`、`TTY-ENDPOINT-001`、`TTY-LOCAL-003/004`。
- UART 唯一拥有 MMIO/IRQ/FIFO/applied line/raw handoff/TX serialization；TTY 不访问 registers。
  console只有output capability，TTY独占RX，capability注册/enable/open/close不改变personality。
- 本阶段不得发布 `/dev/ttyS<N>`、替换 boot fd、修改通用 `CharDev` 语义、建立 controlling relation、
  生成 terminal signal或执行任何 contract cutover。
- 不预建 runtime line reconfiguration、post-publish detach/hangup、generic workqueue 或 PTY transport。

解析触发点：

- Stage 0 关闭后的只读 preflight；根据实际 source matrix 决定 capability 方向、storage、carrier、
  TX route、immutable port identity 和 rollback shape，并精确列出 driver/TTY/kconfig/KUnit write set。

预计范围（不是写入授权）：

- `anemone-kernel/src/device/tty/`、`anemone-kernel/src/driver/serial/ns16550a.rs`、
  `anemone-kernel/src/device/{console,devnum}.rs`、必要的 kconfig/KUnit surface。

## 7. Stage 2 Outline：terminal data plane 与 `TTY-DATA-CUTOVER`

概括目的：

- 落地单一 shared `Terminal`、concrete N_TTY-like discipline、canonical pending/committed input、
  noncanonical `VMIN=1,VTIME=0`、echo/output transform、blocking/nonblocking read 与 iomux readiness。
- 实现正文列出的 termios、control chars、winsize 与 ioctl 下限；按 Stage 0 oracle matrix 冻结
  compatibility bits、invalid combinations 与 errno，不实现 runtime hardware line apply/rollback。
  本阶段提交winsize truth；依赖controlling relation的`SIGWINCH` effect留到Stage 4，不把无target
  时的静默成功冒充完整`TTY-ABI-001`。
- 完成稳定 `ttyS<N>` mapping、major 4/minor `64+N`、专属 devfs open provider与单向 publish；
  console owner 按自己的边界发布 major 5/minor 1 `/dev/console`，但不委托到 `Terminal`。
- 用 real Terminal file 替换匿名 boot fd 0/1/2，审计确认NS16550A raw major 234 `CharDev`不再实现或
  注册，并让 `tty-test` 的 data-plane/termios/readiness matrix成为主要自动oracle。
- 建立专用轻量TTY验收rootfs与显式交互launcher；在用户提供的BusyBox artifact已经完成身份、架构、
  runtime依赖和目标applet核对后，以可自动驱动的smoke和用户执行的vi checklist补充
  `TTY-DATA-CUTOVER`证据。

前置依赖：

- Stage 1 Closed；transport owner、storage、carrier、TX serialization、pre-publish rollback 与
  immutable identity 已有 source/KUnit/runtime 证据。
- BusyBox artifact尚未提供时可以推进Stage 2实现和`tty-test`自动验证，但不得执行Gate P3、
  `TTY-DATA-CUTOVER`或把Stage 2记为Closed。

受保护边界：

- `TTY-TERM-001`、`TTY-INPUT-001`、`TTY-OUTPUT-001`、`TTY-ENDPOINT-001`、
  `TTY-LOCAL-001..005`。
- read/poll 只观察 Terminal committed-input predicate；wake/IRQ/FIFO 不成为 readable truth。
- write 按用户 bytes 计量，`O_NONBLOCK` 从 operation ctx 读取；不缓存 per-file termios/input/nonblock。
- publish 前失败不得留下 node/编号/半绑定 consumer；publish 后不得 runtime unpublish 或编号复用。
- 本阶段不得建立 `/dev/tty` relation lookup、foreground PGID、terminal-generated job-control signal或
  把 shell prompt写成完整 closure。

解析触发点：

- Stage 1 关闭后读取实际 capability、queue/container、module pressure、poll/Event API、VFS copy
  路径、BusyBox termios/vi oracle与endpoint rollback证据，冻结逐文件 write set和data-plane矩阵。

预计范围（不是写入授权）：

- `device::tty` semantic/discipline/file/devfs modules、VFS poll/read/write adapter、devnum、console-owned
  `/dev/console` publication、boot stdio、`tty-test` app、轻量TTY rootfs/launcher与定向userspace oracle。

Contract cutover：

- 本阶段是唯一 prospective `TTY-DATA-CUTOVER` owner。cutover 必须原子更新
  `TTY-PORT-001`、`TTY-TERM-001`、`TTY-INPUT-001`、`TTY-OUTPUT-001` 与 `TTY-ENDPOINT-001`；
  任一项验证不足时整个 unit保持 Not Cut Over。
- cutover 不激活 `TTY-REL-*`、`TTY-JOBCTL-*`、`TTY-LIFE-*` 或 `TTY-ABI-001`，不关闭 RFC。

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

## 11. Stage 0 -> Stage 1 Implementation Resolution Gate

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

- 完整Stage 1与manifest冻结后才达到`Ready`；仍需另行用户授权，不自动开始代码实现。

后续`1 -> 2`、`2 -> 3`与`3 -> 4` resolution gate遵循同一协议，并额外读取前一Stage实际diff、
runtime evidence、bridge状态、module pressure与cutover结果。每次只解析下一个Stage，并按第4节为
预期的多checkpoint Stage冻结完整checkpoint map；不提前解析更远Stage的checkpoint细节。

## 12. Probe / Vertical Slice Gates

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
`SIGHUP`/`SIGCONT`列为通过条件；证据区分source、KUnit、build、agent-run QEMU、user-run与未运行。

**Failure Signals:** BusyBox artifact架构/runtime/applet不满足验收输入、launcher没有建立真实session/
controlling-terminal/foreground序列、shell prompt被当作closure、fake ioctl、raw mode仍signalize、
foreground reclaim失败、session外signal、test-only fallback、人工结果替代自动matrix或延期项被伪装成通过。

**Write-back / Exit:** 分层证据进入transaction，明确记录BusyBox artifact identity、轻量rootfs/launcher、
agent-run与user-run结果；人工发现的稳定语义缺陷尽可能先转为自动回归。只在对应unit全部proof满足时
cutover，否则保持Not Cut Over并按target范围分类open issue、limitation或Target Renegotiation。

## 13. 旁路审计清单

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

## 14. 可观测性清单

后续Ready stage必须按实际路线冻结：

- per-port RX bytes、overflow/drop、line-error、notification、drain batch/budget与no-lost-work assertion；
- endpoint identity/name/devnum、create/publish结果、rollback原因与duplicate rejection；
- termios unsupported/normalized/ignored字段的限频诊断，禁止每字节或每IRQ日志；
- input predicate/read/poll source一致性assertion、canonical boundary与flush计数；
- relation acquire/detach/generation mismatch/retry、foreground change、decision分类和signal target identity；
- bridge启用次数、raw endpoint结论与probe-only watchdog删除证据；
- cutover时每个contract ID、验证provenance、Not Cut Over项与register disposition。

纯诊断字段不得驱动state machine；一旦字段参与行为，必须按协议状态进入owner/不变量/review。

## 15. 全局停止边界

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

## 16. 实现期反馈与记录

2026-07-23 Stage 0 carrier审计确认现有`KThreadHandle::wake()`底层仍受register中的scheduler
IRQ-off allocation问题影响。用户按owner边界决定：TTY直接使用现有wake capability，不因此停摆或
发明workqueue/softirq/专用scheduler路径；该共享问题继续由wait-core/scheduler owner修复。本文已把
这项处置折回Stage 0反馈假设、停止条件和全局边界；它不改变R0 target、owner、ABI、cutover或
acceptance，不产生revision bump或Ready-manifest扩展。

- Execution Fact、checkpoint、review与验证只追加到transaction。
- Route Correction若保持target，只更新本文future Outline/Ready和transaction，不增加RFC修订。
- target/owner/ABI/visible semantics/acceptance变化先更新RFC review与tracking；接受后才形成新revision。
- current contract只在两个approved cutover gate原子更新；Draft RFC、probe与partial candidate不能生效。
