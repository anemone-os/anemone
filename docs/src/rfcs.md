# 公开草案与 RFC

公开 RFC 只用于已经进入共享决策、且涉及 owner、ABI、shared contract、非平凡生命周期/并发、多个 cutover、probe 或 target 风险的方案。Patch 默认没有过程文档；值得长期保存但边界局部的判断使用小迭代记录。完整分级见[开发工作流](./development-workflow.md)。

## 默认形状

RFC 默认只有一个 canonical 入口：

```text
docs/src/rfcs/<short-slug>/
  index.md
```

`index.md` 保存 accepted target、non-goals、owner/handoff/failure/cleanup、ABI/visible semantics、实际 contract delta、Implementation Boundary、acceptance、validation 和 closure。

只有出现真实需要时才增加 `invariants.md`、`implementation.md`、`tracking-issues.md` 或 `backgrounds/`。positioning/backgrounds 不是 RFC 前置步骤；target 已经闭合时直接编写 `index.md`。transaction 也不是 RFC 实现的默认产物；只有长期 RFC、多 checkpoint、多 cutover、probe/renegotiation 证据需要独立执行历史时才创建。具体形状见 [RFC 模板](./rfc-template.md)。

## Target、current contract 与 Git

- RFC target 在 cutover 前不能覆盖[当前契约](./contracts.md)。
- `Contract Impact` 只列 `Introduce`、`Refine`、`Replace`、`Remove`、`Scoped Exception`；未变化规则作为 Dependencies 链接，不登记 `Preserve` 流水。
- RFC 单向链接 current baseline；current contract 不为 pending proposal 维护默认 backlink。
- Git 保存物理文本历史；RFC `R0`、`R1` 只标记 Closed 前已接受的目标、owner、ABI、contract 或 acceptance 语义变化。
- 状态使用`Draft`、`Accepted`、`Review Hold`、`Closed`、`Superseded`、`Terminated`；它不代替用户对当前
  实现任务的授权。`Terminated`表示维护者永久取消未满足acceptance/closure的RFC：无active gate、无current
  contract、不得恢复。未来相关工作必须独立重新分类并取得新的授权/Implementation Boundary；只有仍命中RFC
  分级时才新建RFC。
- `Closed`是不可重新打开或修订的完成终态。RFC目录在closure后冻结为历史资料，不再增加修订、gate或续接
  transaction；生效共享规则由current contract拥有。后续工作从live source、current contract和register重新建立
  独立边界并按当前规则分类，旧RFC内要求未来“回到本RFC”、修订或建立follow-up RFC的措辞不具有流程权威。

## 实现与反馈

Implementation Boundary 约束 target、owner、handoff、ABI、contract、acceptance 和 validation，不冻结逐文件 write set。同 owner 的内部 import/module 注册、新文件、定向测试和行为保持型拆分可以自然闭合；越过语义边界时必须停止。

普通实现、commit 和单次 RFC closure 不需要 transaction，也不需要 resolution/activation/closure 三段式状态。只有独立安全的 checkpoint、contract cutover、probe、不安全中间态或明确人工授权点才建立 gate。用户只授权一个 gate 时，完成后不得自动进入下一 gate。

每次实现收口前都进行架构摩擦扫描。没有具体摩擦或只剩 Safe 时不写占位结论；Euclid 在仍残留时简短报告；Keter/Apollyon 必须在完成声明或 cutover 前停止。不要建立 `friction.md` 或全局摩擦台账。

RFC Closed 前，实现反馈可以在 accepted target 内修正路线；改变 target invariant、owner、ABI、contract、acceptance 或 validation claim 时，必须停止并由 RFC review 决定 Route Correction、Accepted Reduced Target、Follow-up RFC 或 Not Cut Over。agent 不能批准自己的 reduced target。Closed 后的发现属于新的独立任务，不再触发原 RFC 的 review 或 revision。

## 导航与历史

新 RFC 更新本页、`docs/src/SUMMARY.md` 和 RFC 内必要链接，使页面可达；导航只提供链接与范围，不复制阶段、验证和问题状态。既有 RFC、transaction、manifest 和历史状态作为 legacy history 保留，不批量迁移。

旧文档可能仍出现 `Accepted for Implementation`、Ready/Active/Closed、逐文件 manifest、强制 transaction、`P0/P1/P2/P3`，以及要求未来修订原 RFC / 建立 follow-up RFC 等历史形状；它们说明当时流程，不覆盖当前[开发工作流](./development-workflow.md)。

## 当前 RFC

### 其它领域

- [RFC-20260814-static-sysfs](./rfcs/static-sysfs/index.md)：Accepted R0；接受注册 canonical `sysfs`
  no-device filesystem，以 persistent singleton static tree 和 `/sys/kernel/{address_bits,cpu_byteorder}`
  两个只读文本 consumer 原子验证目录、读取与 multi-mount lifetime；明确不接入现有 kobject、动态
  namespace、device model 或 loop sysfs。实现代码参考 procfs 按职责目录化，但不复用 procfs private
  entry，也不提前抽取 generic pseudo-filesystem framework；Accepted 状态本身不构成实现授权，当前也
  没有 contract cutover。
- [RFC-20260810-pty-devpts](./rfcs/pty-devpts/index.md)：Accepted R3；在已关闭的Serial TTY R1之上接受user-mountable
  single-persistent-instance Unix98 PTY/devpts、由devfs预发布且由persistent init挂载的canonical `/dev/pts`、任意已有
  directory上的additional view、dynamic slave semantic endpoint、safe-reuse pair/opened-description lifecycle、
  Linux-default ABI与master-hangup协议；generic dentry freshness继续由VFS register独立拥有，不形成PTY额外Stage或
  owner-local workaround。
  [目标与不变量](./rfcs/pty-devpts/invariants.md)展开owner/lifecycle proof，[实施路线](./rfcs/pty-devpts/implementation.md)
  组织Stage 1--4与全局实现输入，普通PTY Rust test app使用`anemone-rs`并参照`socket-test`形状；pre-RFC定位已归档为
  [背景材料](./rfcs/pty-devpts/backgrounds/index.md)。tmux为
  建议性必试、sshd不进入验收标准；Stage 1--3已关闭且未公开PTY namespace，Stage 4 public activation/acceptance仍为
  Future，当前没有Stage执行授权或contract cutover。
- [RFC-20260809-user-tlb-residency-targeting](./rfcs/user-tlb-residency-targeting/index.md)：Closed / R1；为全部user
  page-table activation建立唯一residency handoff，使destructive TLB shootdown在稳定状态只覆盖仍可能观察旧translation
  的CPU，同时保留现有同步ack、retirement与dependent continuation边界。
- [RFC-20260808-user-tlb-completion](./rfcs/user-tlb-completion/index.md)：Closed / R2；把 user address-space
  remote fence从每次fault的无条件Drop broadcast收敛为completion ordering：monotonic PTE change不创建自己的
  remote obligation，destructive commit的dependent continuation/exposure与retired cleanup均在锁外remote ack之后；
  `MM-TLB-LOCAL-001`已Refine、`MM-TLB-REMOTE-001`已Introduce。
- [RFC-20260803-clock-timekeeping-posix-timers](./rfcs/clock-timekeeping-posix-timers/index.md)：R0 已实现并关闭；建立从硬件计数和
  Hertz 计算的 monotonic/raw、由内存偏移得到的 realtime、按真实更新周期计算的 coarse clock，以及可物理删除
  排队请求的 soft timer；目标是在 RV64/LA64 native 64 位 ABI 上实现五个 clock syscall 和五个 POSIX timer
  syscall，RTC 只作为未来启动时的一次只读时间来源。正确性规则见[目标与不变量](./rfcs/clock-timekeeping-posix-timers/invariants.md)，
  Gate 0--6 的依赖、cutover 和验证见[实施计划](./rfcs/clock-timekeeping-posix-timers/implementation.md)。已生效的
  clock read、realtime step、soft timer request与POSIX timer规则见[Time当前契约](./contracts/time/index.md)，
  `SI_TIMER` pending refine见[Signal当前契约](./contracts/signal/pending-routing.md)，执行证据见
  [transaction](./devlog/transactions/2026-08-04-clock-timekeeping-posix-timers.md)。
- [RFC-20260804-posix-timer-thread-id-notification](./rfcs/posix-timer-thread-id-notification/index.md)：Accepted R0；
  作为已关闭 Clock/POSIX Timer R0 的独立 follow-up，为 native `timer_create()` 增加同 `ThreadGroup`
  exact-task `SIGEV_THREAD_ID` notification。timer owner 保持不变，Signal owner 增加 task-private
  per-registration `SI_TIMER` slot；raw `SIGEV_THREAD` 与 kernel-side callback execution 仍明确排除。
  [目标与不变量](./rfcs/posix-timer-thread-id-notification/invariants.md)定义 target/exit/delete/lock proof，
  [实施路线](./rfcs/posix-timer-thread-id-notification/implementation.md)包含当前已授权Gate 0与三个后续Gate。
- [RFC-20260801-exception-userptr-access](./rfcs/exception-userptr-access/index.md)：R0已实现并由用户验收关闭；
  RV64/LA64通过page-bounded bytewise assembly、per-CPU exact-PC recovery window和一次page-fault retry提供
  fallible copyin/copyout，typed exact access与VFS partial progress边界已经固化。早于本RFC的
  `RemoteUspFenceGuard`锁内同步shootdown问题后来由User TLB Completion RFC关闭；R0 closure仍保留其历史边界。
- [RFC-20260801-loongarch-lsx-context](./rfcs/loongarch-lsx-context/index.md)：R0已实现并关闭；以per-task
  sticky-lazy policy和唯一interleaved trapframe backing保护32个128-bit LSX register及共享FCC/FCSR，
  clone/exec与Linux-compatible signal extcontext已闭合，2K1000实机验收由用户确认通过。LASX、
  `AT_HWCAP*`和Linux full-lazy owner优化保持明确非目标；software unaligned access问题独立登记。
- [RFC-20260803-icmp-raw-socket](./rfcs/icmp-raw-socket/index.md)：R0已实现并关闭；交付privileged
  `AF_INET + SOCK_RAW + IPPROTO_ICMP`、bind/connect、send/receive/read/write、blocking/iomux、`IP_TTL`、`IP_TOS`、
  `ICMP_FILTER`与原始IPv4 RX字节。R0只覆盖unicast、未分片packet；`IP_HDRINCL`、任意protocol、broadcast/multicast、
  message ABI与error queue保持非目标。Checkpoint 1反馈间章、Checkpoint 2A consumer review与Checkpoint 2B ABI/product
  evidence均已关闭；双架构focused 10/10、BusyBox ping 1/1、glibc/musl curated Socket LTP 6/6及最终独立复审共同完成
  `ICMP-RAW-CUTOVER`。四项common contract已Refine、三项ICMP raw contract已Introduce；transaction与register无占位变化。
  [目标与不变量](./rfcs/icmp-raw-socket/invariants.md)与[实施路线](./rfcs/icmp-raw-socket/implementation.md)保留target、
  owner、evidence和一项非阻断guest saturation/retry组合证明Euclid；Stage 1到此结束，不进入后续gate。
- [RFC-20260801-socket-abstraction-and-unix-socket](./rfcs/socket-abstraction-and-unix-socket/index.md)：R1 Closed；以现有
  IPv4 UDP与新增filesystem pathname `AF_UNIX + SOCK_STREAM`作为两个真实consumer，定义最小general Socket front、
  family-owned operation predicate、Unix namespace/stream/lifecycle owner边界，以及bind跨VFS publication的诚实工程
  退路。首版已明确排除`SO_ERROR`、Unix pending-error与error readiness；
  [目标与不变量](./rfcs/socket-abstraction-and-unix-socket/invariants.md)中的六组ABI/lifecycle行为作为scoped Linux
  6.6.32 conformance与实现期validation surface，不再构成R0 Review Hold；多阶段顺序、最终evidence与停止边界由
  [实施路线](./rfcs/socket-abstraction-and-unix-socket/implementation.md)记录。Stage 4已经以双架构、双libc、
  pathname真实consumer与UDP regression完成`SOCKET-UNIX-CUTOVER`，八个Socket/Unix ID及三项iomux/epoll Refine
  同时生效；hardware、`smp>1`、full socket/network LTP与final harness保持Not Run。
- [RFC-20260731-vfs-make-node](./rfcs/vfs-make-node/index.md)：R2 Closed；以 canonical
  `mknodat(33)`、`InodeOps::make_node`、ext4/ramfs有序publication与filesystem-backed `rdev`
  形成完整node-creation target。R2分支内接受的no-umask边界属于历史closure；合流后的current implementation
  复用既有task filesystem context唯一owner，与`openat`/`mkdirat`一致对`mknodat` requested permission应用
  process umask，见[umask小迭代记录](./devlog/changes/2026-07-27-umask-file-creation-mask.md)。
  Stage 1与`DEVICE-NUMBER-CUTOVER`已把
  [`DEVICE-NUMBER-001`](./contracts/device/device-number.md) Refine为effective 12/20 category-neutral baseline；
  Stage 2的`VFS-MAKE-NODE-CUTOVER`又使
  [`VFS-MAKE-NODE-001`与`VFS-SPECIAL-NODE-RDEV-001`](./contracts/vfs/make-node.md)及mount `ENOTBLK` Refine
  effective。R2要求backend-local final metadata
  先于dirent publication、正常并发不可见中间态、成功reload与可实施cleanup；lwext4任意I/O failure/crash
  atomicity由[accepted limitation](./register/current-limitations.md#ane-20260801-vfs-make-node-lwext4-atomicity)
  和后续lwext4/Rust wrapper事务负责。若既有common-create backend/cache/dentry窗口需要新跨ownertransaction，
  则由[独立open issue](./register/open-issues.md#ane-20260801-vfs-create-publication-atomicity)承接，不阻塞本RFC。
  Stage 1-2与C1-C3均Closed；双架构runtime、probe退出、final review与cutover证据见
  [completed transaction](./devlog/transactions/2026-07-31-vfs-make-node.md)。
- [RFC-20260731-posix-record-lock](./rfcs/posix-record-lock/index.md)：R0已实现并关闭；本地`S_IFREG`支持native
  RV64/LA64 POSIX process-associated byte-range record lock，file-table episode拥有holder identity，inode-associated
  VFS domain拥有range/conflict/wait truth，任意相关fd removal执行窄cleanup handoff；现有`flock`与
  opened-description lifecycle保持独立。最终双架构均为288/288 KUnit、19/19 focused、双libc LTP 4/4与384个
  TPASS；LA64在orderly shutdown后因已知缺少power-off driver停在末尾halt，不宣称machine power-off capability。
  `POSIX-LOCK-CUTOVER`已原子激活四项task/VFS contract ID，最终review四级finding全0。完整target delta、
  proof obligations 与滚动阶段见 [目标和不变量](./rfcs/posix-record-lock/invariants.md)及
  [实施计划](./rfcs/posix-record-lock/implementation.md)，执行证据见
  [事务日志](./devlog/transactions/2026-07-31-posix-record-lock.md)。
- [RFC-20260728-flock](./rfcs/flock/index.md)：R0已实现并关闭；opened-description持有、inode-associated VFS
  domain统一裁决的本地whole-file advisory flock支持generic local default并保持record-lock conflict namespace独立。
  final close删除holder grant并提交cooperative recheck hint，不承诺precise close/signal/restart winner。
  RV64/LA64 developer-run acceptance均通过264项enabled KUnit、11项focused oracle与双libc五项flock LTP；
  LA64末尾halt由已登记的power driver缺失解释。`FLOCK-CUTOVER`已把
  [`OPENED-DESC-RETIRE-001`](./contracts/task/opened-description-lifecycle.md)与三个
  [`FLOCK-*`](./contracts/vfs/flock.md) ID原子切换为Effective，执行证据见
  [transaction](./devlog/transactions/2026-07-29-flock.md)。
- [RFC-20260726-net-frame-path](./rfcs/net-frame-path/index.md)：R1的Stage 1-3与`NFP-FINAL-CUTOVER`历史closure
  保持；单一Stage 4已修正registry pending capability owner、post-`Late` boot order与host-test metadata，
  NFP-008/009/010同步neutralize，R1重新Closed。六个Network ID和`SYSTEM-POWER-ORDERLY-001` Refine继续
  Effective；Stage 4 contract cutover为None，未续写原Completed transaction或修改current contracts。
- [RFC-20260729-net-udp](./rfcs/net-udp/index.md)：R0已实现并关闭；Stage 1/2分别建立initial-domain唯一Stack、
  logical interface/static IPv4 control plane与production local path，Stage 3/4完成Endpoint/File lifecycle、
  unconnected UDP transaction、blocking与poll/select/epoll、capacity/copy-fault/fragment证据。Stage 5以同源guest
  case和bounded host peer完成RV64/LA64 remote-external双向proof；两项获批LA64 Route Correction修复PCH-PIC/
  EIOINTC delivery与同步VirtIO block空IRQ handler，保持原owner与R0 target。最终双架构均为274/274 KUnit、UDP
  17/17、epoll 11/11、LTP 4/4、peer PASS与orderly shutdown；LA64无电源驱动时在halt后由monitor `quit`收尾。
  `NET-UDP-FINAL-CUTOVER`已原子使四项[UDP Socket contract](./contracts/net/udp-socket.md) Active；hardware、
  `smp>1`、full network LTP与final harness保持Not Run。执行证据见
  [transaction](./devlog/transactions/2026-07-29-net-udp.md)。
- [RFC-20260804-udp-socket-extension](./rfcs/udp-socket-extension/index.md)：IPv4 UDP connected、
  file/message/vector I/O、flag边界与Endpoint owner/lifecycle；R1与Stage 1/2均已关闭，
  `UDP-EXT-R1-CUTOVER`已Refine三项current contract。两架构当前musl工具链构建的repository-owned C consumer、
  未修改musl resolver与临时external acceptance均通过；临时host orchestration已删除，glibc resolver保持
  Not Supported / Not Cut Over，physical hardware、`smp>1`、full network LTP与final harness保持Not Run。
- [RFC-20260805-net-tcp](./rfcs/net-tcp/index.md)：Closed R0 / Stage 1--5 Closed / TCP Effective；目标为
  initial-domain IPv4 TCP Socket capability，并以同一closure同时验证TCP target与既有网络架构封顶。
  TCP也是Socket framework的反馈consumer：自然shared obligation经RFC review回到共同owner，不能为维持
  current shape塞入TCP-local hack，也不预建没有真实复用义务的generic framework。
  Stage 1已通过`NET-PROTOCOL-PROGRESSION-CUTOVER` Refine `NET-CONTROL-PLANE-001`与
  `NET-STACK-PUMP-001`，把UDP、ICMP raw及future TCP统一到各protocol owner驱动、既有worker承接的
  progression handoff，但不共享effect policy，也不冻结effect/wake表示、存储、锁或worker拓扑。
  crate-only P0已证明窄async cause与bounded listener composition路线，临时probe与feature启用均已删除。
  Stage 1已交付production handoff、UDP/ICMP raw原子迁移与Stack-private TCP foundation；双架构focused回归通过，
  LA64只证明完整shutdown顺序、不宣称wrapper exit 0。Stage 2的CKPT 2A/2B已分别关闭：先建立kernel窄TCP owner
  capability，再接入syscall-unreachable Socket front。Stage 3的CKPT 3A已闭合Stack owner fact/lifecycle，CKPT 3B
  已完成仍不可达的Socket/ABI completion与同owner TCP family目录化；RV64/LA64 release build、RV64 KUnit `459/459`、
  既有Socket consumer回归与独立review通过。CKPT 4A随后以Stack owner facts、recheck-only invalidation、weak reverse
  route和shared production source闭合TCP readiness/connect/accept wait与lifecycle；host TCP owner `20/20`、focused
  smoltcp TCP `178/178`、RV64 KUnit `465/465`、RV64/LA64 release build与独立复审通过。CKPT 4B通过唯一normal
  resolver发布完整TCP creation tuple，闭合connect errno、`SO_ERROR` single-consumer/rearm与binding/local分离；
  exact-source RV64 KUnit `466/466`、双libc TCP oracle、socket LTP `8/8`、shared regression、LA64 release build与
  最终独立review通过。Stage 5随后以单一Checkpoint 5A完成长期`socket-test` TCP suite、双架构双libc focused
  consumer、self/remote-external、established RST、CAgent、shared regression与architecture capstone；独立review
  最终为`0 Apollyon / 0 Keter / 0 Euclid / 0 Safe`。`NET-TCP-CUTOVER`已Introduce三项TCP current contract并Refine
  `SOCKET-ABI-001`，transaction None；physical hardware、`smp>1`、其它NIC/platform、full network LTP、完整final
  harness与条件性deployment probe保持Not Run。
  当前发布正文、[目标与不变量](./rfcs/net-tcp/invariants.md)、[实施计划](./rfcs/net-tcp/implementation.md)及冻结的
  [历史定位共识](./rfcs/net-tcp/backgrounds/positionings.md)；current effective规则见Network与Socket contract，
  register没有新增当前问题。
- [RFC-20260809-read-only-network-diagnostics](./rfcs/read-only-network-diagnostics/index.md)：Closed R0；交付
  initial-domain、IPv4-only、request-time snapshot的只读netlink子集，使未修改`ip link/addr/route show`与
  `ss -tan`可读取logical interface、static control plane与TCP owner facts。既有网络owner不迁移；新增netlink
  transport只拥有port、budget state与bounded pending reply work，诊断snapshot不得反向驱动网络行为。
  `NETLINK-DIAGNOSTICS-CUTOVER`已Introduce三项netlink current contract并Refine `SOCKET-ABI-001`；双架构
  KUnit、raw oracle与未修改BusyBox/iproute2工具通过，独立review为全0，supporting pages与transaction未创建。
- [RFC-20260726-system-power](./rfcs/system-power/index.md)：R0 已实现并关闭；`power` 唯一拥有 terminal
  episode，orderly 当前以静态 `filesystem -> network -> device -> machine` plan fail-forward，panic/emergency 跳过
  ordinary plan并共用 machine-handler fallback。四个 ID 已原子写入
  [System Power current contract](./contracts/power/shutdown-lifecycle.md)，执行与 architecture coverage见
  [transaction](./devlog/transactions/2026-07-26-system-power.md)。
- [RFC-20260726-epoll](./rfcs/epoll/index.md)：R2 已实现并关闭；以 source-neutral persistent
  readiness subscription 统一 poll/select 与 epoll 的 source-facing protocol，并由 `Epoll` / `EpollWatch`
  单独拥有 watch、generation、policy、bounded scan与ET dirty causality。Stage 0-1 已完成并关闭；eventfd/fanotify 已迁移
  到 source-neutral route，source bridge 已删除，subscription 与 opened-description 两个 foundation
  cutover 已同步生效。2A dormant watch/lifecycle core、R0 2B ready/wait protocol与2C ABI/focused oracle已关闭；
  2D首次RV64 runtime发现epoll-file在active wait内获取sleepable mutex并panic。R1以per-instance operation BKL
  的bounded scan取代ready queue/COW/sequence，并用三态coverage + fixed routes形成non-sleeping wait publication；
  2R已按独立授权完成协议修正、两架构build、RV64 focused runtime与review。第二次2D runtime证明
  `epoll01`稳定触发MM COW shadow ancestry stack overflow；R2将该fork-stress case移出epoll验收并登记
  MM Apollyon。修订后的matrix又以`epoll_wait02`命中shared timeout early wake、以`epoll_wait06`命中pipe
  capacity/atomic-threshold缺口；批准的timer与pipe owner修复使双root closure全部通过。`EPOLL-CUTOVER`
  已使epoll ABI、`IOMUX-POLL-001/002` Refine与三个`EPOLL-*` ID同步生效；current truth见
  [Epoll contract](./contracts/epoll/protocol.md)。执行证据见
  [Epoll 事务日志](./devlog/transactions/2026-07-26-epoll.md)。
- [RFC-20260722-system-target-model](./rfcs/system-target-model/index.md)：R6已实现并关闭；QEMU参数化统一为具名opaque-string bind并允许optional runtime argv group，两种initial-program source支持完整argv。决赛脚本与具体决赛配置不在RFC。R0-R5历史均保持关闭；[`BOOT-PROTOCOL-001`](./contracts/task/boot-protocol.md)已在R6A原子Refine。
- [RFC-20260723-ahci-controller](./rfcs/ahci-controller/index.md)：PR #136带入的generic AHCI 1.x、
  ATA block facade与2K1000 platform integration历史入口。未接受Draft已Terminated，不再形成active gate或
  current contract；既有实现/证据保留，live lifecycle/capacity defect与可见限制以register为准。
- [RFC-20260720-unix-jobctl](./rfcs/unix-jobctl/index.md)：R1已实现并关闭；`UJ-CUTOVER`将ThreadGroup-owned stop/continue phase、mandatory user-entry barrier、stopped/continued child report、Signal control ordering与procfs projection作为同一个integrated unit切换为[current contract](./contracts/task/job-control.md)。TTY relation与terminal policy现已由[TTY job-control contract](./contracts/tty/job-control.md)接入；orphaned-pgrp、ptrace、`si_uid = 0`与SIGCHLD publication order继续由register跟踪。
- [RFC-20260716-dw-mshc-sd-cold-discovery](./rfcs/dw-mshc-sd-cold-discovery/index.md)：Accepted / Runtime Validation；固化 protocol-neutral DW-MSHC host、one-shot SD Memory discovery、typed card bus、`mmcblkN` endpoint 与 VisionFive 2 whole-disk `mmcblk0` ext4 rootfs 边界。两轮 correctness findings 已修复，firmware/String/rootfs input 按用户决定完成边界处置，canonical RFC 已同步；实机 attach/read/write/rootfs 仍待验证。
- [RFC-20260714-cpu-logical-physical-id](./rfcs/cpu-logical-physical-id/index.md)：已实现并关闭；platform `MAX_PHYS_CPU_ID` 与 kconfig `MAX_LOGICAL_CPUS` 分开约束物理 ID backing 和最大启用逻辑 CPU 数，固定 per-CPU 表使用槽位内建 `CachePadded<T>` 的 `CpuTable` / `PhysCpuTable` 编码索引域与缓存布局。VisionFive 2 容量修正由用户复验通过，最终 table 布局与 LoongArch correction build 未由 agent 运行。
- [RFC-20260629-vfs-direct-user-io](./rfcs/vfs-direct-user-io/index.md)：已实现第一版；定义普通文件 `read` / `readv` / `pread*` 与 `write` / `writev` / `pwrite*` 的 direct userspace copy 边界、VFS-owned user-buffer cursor、fanotify transaction adapter，以及 ramfs/ext4 regular file read/write hook。`RWF_*`、完整 Linux `O_DIRECT`、mmap coherency、splice family 和 non-regular backend hook 仍按 register / follow-up 边界处理。
- [RFC-20260620-threaded-timer-event](./rfcs/threaded-timer-event/index.md)：已实现第一版；定义 soft timer 的 threaded completion lane、per-CPU timer worker、通用 `Late` initcall、`timerfd` / `ITIMER_REAL` 迁移边界，以及 wait-core timeout 非目标。
- [RFC-20260618-sched-wait-preempt-arming](./rfcs/sched-wait-preempt-arming/index.md)：阶段 3 已关闭；定义 wait-core 在 kernel preempt 下的 wake-prerequisite / parkability contract、scheduler entry split、preempt-defer、token-bound wait sleep、single-active-wait 诊断和 feedback routing 边界；未运行的 trace / fairness evidence gap 见事务日志。
- [RFC-20260711-sched-rt-class](./rfcs/sched-rt-class/index.md)：R0 已完成共享 `Realtime` class、FIFO/RR policy、typed priority、99 个 priority bucket 与 RR quantum；R1 已由 `39ba07a9` 完成并关闭，删除 class-visible resched cause continuation，以 RR-owned `rotation_due` 表达 committed rotation，并把 pending 收窄为 processor-owned single bit。R0 证据见 [2026-07-12-sched-rt-class](./devlog/transactions/2026-07-12-sched-rt-class.md)，R1 证据见 [2026-07-14-sched-rt-class-r1](./devlog/transactions/2026-07-14-sched-rt-class-r1.md)。
- [RFC-20260713-sched-fair-stride](./rfcs/sched-fair-stride/index.md)：已完成第一版；稳定 `Fair` class identity、经典 fixed-tick Stride、Linux nice weight、最小 pass heap、placement floor、yield handoff与 compile-time default selector已经落地。用户完成 Fair default `all` LTP profile和 `fair-test`，并接受相对 RT/RR约 13–14%的同量级耗时差距；IRQ-off heap allocation继续由 register跟踪。事务证据见 [2026-07-13-sched-fair-stride](./devlog/transactions/2026-07-13-sched-fair-stride.md)。
- [RFC-20260714-sched-dynamic-attributes](./rfcs/sched-dynamic-attributes/index.md)：R1 已完成并关闭；scheduler-owned config patch与固定owner-CPU `RunQueue` transaction统一动态nice、Fair/RT policy、RT priority、reset-on-fork与fixed-CPU affinity，async IPI和persistent-phase one-shot completion保持同步syscall语义。R1 的临时全局remote submission gate 已由 [SCHED-WAKE 当前契约](./contracts/scheduler/wake-delivery.md)取代并删除；历史实现、验证、Not Run与最终review证据见 [Completed事务](./devlog/transactions/2026-07-15-sched-dynamic-attributes.md)。
- [RFC-20260616-kthread-core](./rfcs/kthread-core/index.md)：已接受、阶段 6 implementation gate 已关闭；纠偏 kthread core，定义 procfs-visible singleton thread group、固定 `kthreadd` TID 2、strong handle、专用 exit、user-facing API fail-closed，以及移除 service/park 的迁移 gate。
- [RFC-20260614-kthread](./rfcs/kthread/index.md)：历史基线；记录已落地的轻量 kthread 创建代理、typed entry、stop/park 生命周期和 `KThreadService` 后台 worker 合同，已由 `kthread-core` supersede。
- [RFC-20260614-inode-shrinker](./rfcs/inode-shrinker/index.md)：自循环 `io_shrink_threshold` gate 的 inode cache shrinker、superblock eviction path 和 ext4 backing file cache 计数合同。
- [RFC-20260615-oom-killer](./rfcs/oom-killer/index.md)：Terminated历史proposal；allocation-success wake target未闭合runtime acceptance，当前OOM trigger与policy由[`MM-OOM-001`](./contracts/mm/oom-policy.md#mm-oom-001--oom-worker自有fixed-delay采样与victim-round)定义。
- [RFC-20260602-cred-merge](./rfcs/cred-merge/index.md)：credentials feature merge 的 canonical 执行计划和审查合同。
- [RFC-20260606-signal-temp-mask-restore](./rfcs/signal-temp-mask-restore/index.md)：`rt_sigsuspend`、`ppoll`、`pselect6` 临时 signal mask delayed restore 协议、trap-return delivery handoff 和 staged 实施计划。
- [RFC-20260605-fileops-seek-char-ioctl](./rfcs/fileops-seek-char-ioctl/index.md)：`FileOps::seek`、positioned I/O 分层和字符设备 ioctl 默认分发计划。
- [RFC-20260604-fanotify](./rfcs/fanotify/index.md)：fanotify path-fd 通知、group fd、mark registry 和 staged LTP 兼容计划。
- [RFC-20260604-mount-tree-legacy-api](./rfcs/mount-tree-legacy-api/index.md)：第一版已实现并完成阶段 7 收口；保留 shared/slave/unbindable propagation、mount flag matrix、fstype alias bridge、ROFS mmap/writeback 和 unmount cleanup 等 register limitations。
- [RFC-20260604-proc-tgid-fd](./rfcs/proc-tgid-fd/index.md)：`/proc/<tgid>/fd` 目录枚举、fd symlink `readlink()` 和第一阶段 procfs/fd 兼容计划。
- [RFC-20260603-sched-latch](./rfcs/sched-latch/index.md)：`poll` / `select` OR wait 所需的 wait-core latch 原语和 iomux 迁移计划。
- [RFC-20260601-sched-wait-refactor](./rfcs/sched-wait-refactor/index.md)：R0 已完成；post-close synchronous remote placement 问题已由 [SCHED-WAKE 当前契约](./contracts/scheduler/wake-delivery.md)取代并 neutralize。

## 已关闭或延期 RFC

- [RFC-20260722-tty-subsystem](./rfcs/tty-subsystem/index.md)：R1已实现并关闭；serial TTY的专属Terminal/FileOps、稳定`/dev/ttyS<N>`与`/dev/tty`、termios/data-plane、controlling relation和BusyBox ash/vi foreground job-control包络均已交付。`TTY-DATA-CUTOVER`与`TTY-JOBCTL-CUTOVER`分别由[data-plane](./contracts/tty/data-plane.md)和[job-control](./contracts/tty/job-control.md) current contract拥有九个Active ID。RV64自动、focused与用户人工证据通过；LA64、hardware和LTP明确Not Run。
- [RFC-20260603-IOCTL-LOOP](./rfcs/ioctl-loop/index.md)：已实现并关闭；完成 `ioctl(2)` VFS 分发、统一 block ioctl、静态 loop 设备池与第一阶段 loop ioctl。扩展 loop sysfs、partscan、direct I/O、autoclear 和 ioctl LTP 缺口继续由 register 跟踪。
- [RFC-20260622-sched-eevdf-lite](./rfcs/sched-eevdf-lite/index.md)：Stage 3/R1 runtime acceptance 失败后延期关闭，不是 Completed；关闭时 default 曾恢复为 RR，后续已由 Fair / Stride 切换为 Fair。EEVDF 保留为可运行实验原型，但显著吞吐回归与百万级 yield self-pick 仍存在，`EEVDF-001` / `EEVDF-018` / `EEVDF-004` / `EEVDF-020` 保持未解决 Keter。事务日志见 [2026-07-09-sched-eevdf-lite](./devlog/transactions/2026-07-09-sched-eevdf-lite.md)，证据见 [Stage 3 eligibility 回归背景](./rfcs/sched-eevdf-lite/backgrounds/stage3-eligibility-regression-20260711.md)。

当一个 feature 被多个 RFC 分段覆盖时，本页可以作为轻量聚合入口，或由其中一个 umbrella RFC 在 `index.md` 中聚合链接。聚合入口只列出相关 current contracts、RFC、按需 transaction、register / current limitations 及其覆盖范围；不要复制规则正文、阶段完成度、验证矩阵或问题状态。

已提取的共享规则以 current contract 为准，accepted target 以对应 RFC 为准，执行事实以 live code、Git/PR 和被 closure/cutover 引用的证据为准。Draft/Accepted RFC 是目标来源，不是当前事实；只有达到 cutover 的验证和停止条件后，长期共享规则才写入 current contract。
