# PTY / devpts 前置定位

**状态：** 已归档的 pre-RFC positioning snapshot；不再维护

**归档日期：** 2026-08-10

**范围：** RFC 地位、首版候选能力与兼容边界、责任拓扑、生命周期承诺与明确非目标

**不包含：** 技术路线、具体类型/接口/锁、可执行状态机、RFC target invariants/proof obligations、stage/checkpoint、
Contract Impact、实现授权与执行证据

本文是 [RFC-20260810-pty-devpts](../index.md) 晋级前的定位快照。它不是 Accepted Target、current contract、
Implementation Boundary 或实现授权；其中候选 profile、未决问题分类和未来时态只反映归档时的讨论状态。
RFC 正文与[目标不变量](../invariants.md)是当前 canonical authority。

## 结论

PTY / devpts 应形成一个独立的 follow-up RFC，暂用 `pty-devpts` 作为 slug。它不重新打开已经关闭并 cut over
的 Serial TTY R1，也不把 PTY 处理成该 RFC 的小补丁。新 RFC 依赖现有 TTY data plane、controlling relation、
job control、iomux/epoll 与 opened-description lifecycle，但只在真实发生变化时提出 contract delta。

下文“首版候选 profile”表示未来 public RFC 的初始目标候选，不是已经 Accepted 的 RFC `R0`。其总体定位是：

- 内核建立完整的动态 semantic TTY endpoint 与 PTY pair 生命周期能力；
- 用户态获得可由仓库内普通PTY test app证明、并可供sshd与tmux消费的单实例Unix98 PTY profile；
- 不追求 Linux PTY 内部实现同形，也不把完整 Linux PTY/devpts 历史语义作为默认兼容目标。

这项工作命中 RFC 分级，因为它同时引入新的用户 ABI、runtime endpoint 与 pathname 生命周期、跨 owner
final-close/hangup、controlling-relation retire，以及 TTY、VFS/devpts 和 `task::files` 之间的非平凡 handoff。
它不能降格为 Patch 或 checkpointed small iteration。

## 兼容原则

首版候选 profile 优先保证真实程序依赖的稳定可观察语义，而不是复刻 Linux 内部对象、锁序、driver 层次或
冷僻历史行为：

- POSIX、Linux UAPI 或目标程序明确观察的行为，应形成清晰 target 或显式限制；
- 用户无法观察、UAPI 不承诺或只属于 Linux 内部实现的细节，不形成兼容义务；
- 偏僻 Linux edge semantics 可以排除或记录为限制，但不得借此弱化资源生命周期、无丢失唤醒、ABI 诚实性、
  唯一状态 owner、内存安全或 cleanup 等 correctness invariant；
- LTP 是兼容性输入和回归来源，不单独定义体系结构。最终core能力必须由仓库内普通PTY test app验证；
  sshd/tmux workload只提供独立归因的集成证据。

## 内核能力边界

### 状态与协议 owner

本 positioning 已形成下列责任拓扑共识；这里的 owner 表示能够推进对应可变真相的唯一领域，不预设 Rust
对象是否一一对应：

| 状态 / 能力 | 唯一 owner | 其它参与方只持有什么 |
| --- | --- | --- |
| committed termios、winsize、line discipline、input/output stream 与数据面 readiness predicate | `Terminal` | serial/PTY attachment 与 opened file 持窄 data-plane capability |
| immutable pair/terminal identity、master liveness、slave lock、slave opened-description participation、peer absence 与 hangup/retire | PTY pair lifecycle owner | master/slave FileOps 持 operation-local capability；wake 只要求 predicate recheck |
| PTY index、live `N -> pair` backend binding、node metadata policy/source 与 logical binding retirement | devpts backend | allocation transaction 持 prepared pair capability；runtime pair 只提交 retirement request；VFS 只接收窄的 lookup/publication/retire facts |
| resident inode metadata/index、dentry cache/materialization 与 mounted pathname visibility | VFS superblock/inode/dentry owner | devpts 提供 backend mapping 与 metadata input；pair 不读取 VFS 私有 cache 或 lock |
| controlling session binding、foreground selector与relation generation | 现有 TTY relation owner | PTY slave 以稳定 terminal identity 参与；pair 只提交 retire request |
| 每个 opened description 的 `Unpublished -> Live -> Retired` 与 final release | `task::files` | PTY pair 接收窄的 enrollment/final-release effect，不读取 fd-table 私有状态 |
| Session/ProcessGroup membership、Signal occurrence/action、ThreadGroup stop/continue/report | 现有 task、Signal 与 job-control owner | TTY/PTY 只提交经重验的 target/effect request |

`semantic TTY endpoint` 是稳定 terminal identity、普通 slave-side TTY capability 与 relation participation 的边界，
不是位于 `Terminal`、PTY pair 和 relation 之外的另一份可变状态 owner。PTY master 不加入 controlling relation；
slave 才是该 pair 对外提供的 semantic terminal endpoint。PTY pair lifecycle owner 属于 TTY/PTY backend，而不是
devpts；devpts 只推进 backend binding、metadata policy 与 logical retirement，不因持有 binding 而取得 runtime pair
truth。VFS 仍唯一拥有 resident inode/dentry、cache publication/materialization 与 mounted pathname visibility。

### 多态边界

TTY 的多态必须保留“semantic TTY endpoint 与 attachment participation”边界，但本文不冻结具体 trait 形状。
PTY 不必实现当前 physical `TtyPort`；RFC 可以保留 serial/PTY 两套窄 attachment，也可以提取新的 backend-neutral
attachment capability，只要不移动下面的 owner truth：

- `Terminal` 继续拥有 termios、winsize、line discipline、slave input/output stream 与数据可用性/容量 predicate；
- semantic TTY endpoint 只提供稳定 terminal identity、普通 slave-side TTY FileOps 与 controlling-relation
  participation，不缓存 pair lifecycle 或 relation truth；
- serial attachment 继续独占物理 port、MMIO/IRQ、raw RX handoff、worker 与 hardware TX；无论保留、重命名还是
  拆分当前 `TtyPort`，PTY 都不能取得这些 physical truth；
- PTY pair 拥有 master FileOps、master liveness、slave opened-description participation、slave lock、peer presence 与
  hangup lifecycle；
- PTY master 不是第二个 `Terminal`，也不是伪造的 physical port；
- devpts 是独立的 dynamic pseudo filesystem backend，只拥有编号、backend pathname binding、metadata policy/source
  与 logical retirement；VFS 拥有 resident inode/dentry 与 mounted pathname visibility。两者都不拥有 termios、
  字节流、pair liveness 或 job-control truth。

预期数据方向是：

```text
master write -> Terminal input / line discipline -> slave read
slave write / echo -> Terminal output             -> master read
```

PTY I/O 的最终 readiness 由两类 durable predicate 无缓存组合：数据可用性/容量来自 `Terminal`，peer
presence/hangup/retire 来自 PTY pair。FileOps 可以组合 owner snapshot 或 capability，但不能保存第三份
readable/writable/hangup truth；notification 只触发完整 predicate recheck。组合注册、recheck 与锁序留待 RFC 证明。

这里形成共识的是责任边界，不冻结 Rust trait、struct、模块布局、锁、buffer placement 或具体回调形状。新增
internal surface 默认保持 owner-private；是否需要共享接口，留给 RFC owner/handoff review 决定。

### 关系与 opened-description 生命周期

- PTY slave 必须能够作为 runtime semantic endpoint 加入和退出现有 controlling-relation protocol；现有 boot-only
  serial slot 形状不能成为 PTY 的生命周期模型。
- Session/ProcessGroup membership、Signal occurrence/action 与 ThreadGroup stop/continue truth 继续由现有 task、
  Signal 和 job-control owner 持有；PTY 不复制这些状态。
- master/slave 的 `dup`、fork、close 与 fd-table teardown 必须服从 `OPENED-DESC-001`：published opened-description
  lifecycle 是 final close 的唯一真相，`Arc::drop`、`File` 内存存活和裸 fd 数量都不能替代它。
- 多次 pathname open 产生多个 slave opened descriptions；同一 description 的 dup/fork aliases 只在最后一个
  published slot 撤销时离开 pair participation。
- pair-owned slave participation 是由成功 enrollment 与 opened-description final release 推进的 peer-presence
  protocol state，不是另一套用于判断 opened-description final close 的 refcount；pair 不得从 `Arc` strong count、
  裸 fd 数量或 pathname/inode 引用反推它。
- PTY backend 所需 final-release effect 必须与现有 opened-description/fanotify lifecycle 自然组合，不能覆盖已有
  hook、让 pair participation 替代 opened-description final-close truth，或顺势引入无需求的动态 observer
  registry。当前 `OPENED-DESC-003` 只有一个 creation-time static hook，普通 pathname open 已用它提交 fanotify
  close effect，因此 composition、exactly-once 顺序、failure/cleanup 与 Contract Impact 必须在 RFC 接受前闭合；
  具体 internal surface 留给 RFC 实现路线决定。

### PTY episode 与跨 owner handoff

- system devpts instance 是 allocation transaction owner：它编排 index/binding 与 prepared pair capability，但不
  读取或推进 pair runtime state。一次成功的 `/dev/ptmx` open 创建一对独立 PTY、一个 master opened description
  与一个初始 locked 的 slave binding；成功返回前，pair、master 与 pathname 必须已经形成自洽的可用 episode，
  随后 runtime lifecycle authority 归 pair owner。任一失败不得留下可发现但不可完成的 pair、可打开 slave、
  relation、participant 或 waiter。
- `TIOCSPTLCK` 只改变 pair-owned slave admission。pathname open 与 `TIOCGPTPEER` 必须进入同一个 admission
  owner，执行同一 live/lock/permission policy，并产生语义等价的 slave opened description，不能形成两套 open
  protocol。
- slave open 与 master retirement 并发时，pair owner 是 admission/retirement 的唯一仲裁点：一次 open 要么在
  retirement 前完整 enrollment，要么失败；不得发布 half-enrolled opened description。
- master 仍 live 时，最后一个 slave opened description final release 只形成 peer absence 并唤醒相关 master
  operation；它不退休 pair。若 slave 仍 unlocked，之后可以重新 open 并重新 enrollment。
- master opened description final release 触发 pair owner 唯一、不可逆的 retirement 线性化点。该点原子禁止后续
  slave admission并发布 pair hangup/retired predicate；随后才由窄、幂等 handoff 请求 devpts retire binding、TTY
  relation owner撤销relation并完成Signal/job-control effect，以及各 wait owner 重验并唤醒。
- 上述跨 owner cleanup 不是一个持有共同锁的全局原子操作。retirement 后的 effect 必须单调、可重复请求且不得
  恢复 live/discoverable 状态；pathname、relation、wait participation、inode/dentry 与 pair storage 的物理回收
  可以更晚完成。

### devpts 实例与编号

- 首版候选 profile 只有一个 system-wide devpts instance；`/dev/pts` 是该独立 dynamic pseudo filesystem 的系统
  mount/view。
  不引入 mount namespace、per-mount PTY instance 或 `newinstance` 语义。
- `/dev/ptmx` 是 devfs 中的静态 character-device allocation 入口，始终路由到该 system devpts instance；它不把
  动态 slave namespace 或 pair lifecycle 下沉给现有 append-only devfs。
- 首版候选 profile 固定 identity-safety 结果而不预选 allocator：retired/stale inode、dentry 或 handle 永远不能
  接入另一对 live PTY。编号单调不复用，或在 VFS freshness/incarnation proof 闭合后安全复用，留待 RFC 比较。
- 若 RFC 选择固定的 live-pair、index 或其它 admission ceiling，该重要容量上限必须进入 Kconfig；quota/index
  exhaustion 时新 `ptmx` open 诚实返回 `ENOSPC`，普通 backing allocation failure 仍返回 `ENOMEM`。本文不要求为了
  形式单独发明 live-pair budget。若 RFC 选择 boot-lifetime 单调编号并允许累计耗尽，必须把该用户可见 tradeoff
  明确纳入 target，而不能作为实现附带结果。
- master final close 后，`/dev/pts/N` binding 立即逻辑退休，fresh lookup/open 不得取得 live slave。现有 VFS
  dynamic positive dentry 问题关闭前，已经缓存的旧 dentry 可以作为首版候选的显式限制继续 `stat` 到 inert inode，
  但 open 必须按 pair liveness fail closed，且绝不能命中新的 pair。若编号复用会让 stale positive dentry 遮蔽
  新 binding，RFC 必须先解决相应 VFS owner/handoff，不能在 devpts 中增加第二套 freshness truth。
- identifier reservation 是否允许失败空洞属于 allocator policy；无论选择何种策略，分配或 open 失败都不得留下
  live pair、可打开 slave、relation、participant 或 waiter。

## 用户可见首版候选能力

### Unix98 分配与发现

首版候选 profile 拟提供：

- character device `/dev/ptmx`；
- mounted system directory `/dev/pts` 与动态 `/dev/pts/N` slave node；
- 每次成功打开 `/dev/ptmx` 创建独立 PTY pair；
- slave 初始锁定，解锁前 pathname open 失败；
- `TIOCGPTN`、`TIOCSPTLCK` 与 `TIOCGPTPEER`；
- `O_CLOEXEC` 与 `O_NONBLOCK` 的正确 generic fd/opened-description 语义；`O_NOCTTY` 的可见效果必须与 RFC 接受的
  slave controlling-terminal acquisition policy 一并决定，在该 policy 未闭合前不能先声称兼容；
- 明确且可验证的 slave uid/gid/mode/stat、permission 与 open-admission profile，以及多个独立 slave open。

首版候选 profile 不要求在内核中模拟历史 grant helper。metadata/permission 必须在 node
创建时直接形成，不得在尚未决定 profile 时先把固定 root metadata 或伪造 grant transition 写成既成
语义。public RFC review使用固定Linux source核验kernel-visible ABI，并由普通PTY test app验证Anemone行为；
userspace helper调用链不是citation authority或acceptance profile。

### Terminal 数据面与 job control

首版候选 target 必须提供一对真实可用的 terminal stream，而不只是能通过 allocation smoke test：

- master write 经现有 Terminal input conditioning、canonical/raw discipline、echo 与控制字符处理后供 slave read；
- slave write 与 echo 经现有 output processing 后供 master read；
- `TCGETS`、`TCSETS`、`TCSETSW`、`TCSETSF`，至少覆盖 raw termios 和 sshd/tmux 所需组合；
- master/slave 观察同一 termios 与 winsize truth，并支持 `TIOCGWINSZ`、`TIOCSWINSZ` 与 `SIGWINCH`；
- slave 复用现有 `TIOCSCTTY`、`TIOCNOTTY`、`TIOCGSID`、`TIOCGPGRP`、`TIOCSPGRP`、caller-relative
  `/dev/tty`、foreground/background access 和 terminal-generated signal；
- `VINTR`、`VQUIT`、`VSUSP` 等输入效果经现有 relation/task/signal handoff 投递到 foreground process group；
- master 与 slave 的 blocking/nonblocking I/O、partial progress、poll/select/epoll readiness 和 waiter wakeup来自
  durable owner predicate，不来自 wake count 或一次性事件。

### Close、peer absence 与 hangup

以下不是冷僻 Linux corner，而是 ssh/tmux session termination 的首版候选必选能力：

- 关闭一个 dup/fork alias 不得让 pair 提前观察 final close；
- master 仍存活时，最后一个 slave opened description 关闭会唤醒 master 并使其观察 peer absence，但不永久退休
  pair；unlock 状态允许后续重新打开 slave；
- master final close 在 pair owner 提交不可逆 retirement，并通过后续 owner-local handoff 禁止新 slave open、
  撤销相关 controlling relation、发布 hangup predicate并唤醒全部 blocked reader/writer/poller；
- hangup 后已经提交的 buffered data、EOF、`EIO`、partial progress 与 poll/epoll bit 必须形成自洽、Linux-reference-backed
  matrix；该 matrix 闭合前，本文只固定“不得永久阻塞、伪造成功或遗漏 HUP/ERR/EOF terminal outcome”；
- PTY master-hangup cause 必须让 relation owner 撤销对应 controlling relation，并执行由 Linux reference 与目标 workload
  共同确认的 signal effect；本文不预先把 recipient 写成 foreground process group。具体 recipient、signal set、
  ordering 及 stop/continue effect 必须在 RFC ABI/effect matrix 中闭合，signal occurrence/action 与 job-control
  transition 仍由现有 owner 提交；这不自动把 `TIOCNOTTY`、session-leader exit、orphaned process group 等全部
  relation-disassociation effect 纳入首版候选；
- pathname、relation、wait participation 与 pair storage 的物理回收可以晚于逻辑退休，但不得恢复可发现性、
  丢失 wakeup 或把旧 identity 重新接入新 pair。

last-slave-close 时 master read 的精确 EOF/`EIO` 选择、hangup 后纯查询 ioctl、poll bit 组合与 buffered-data
precedence 仍需进入 RFC ABI validation matrix；这不改变上述 lifecycle capability 必须属于首版候选。

## 明确非目标

首版候选不包含：

- legacy BSD PTY name/device；
- multiple devpts instances、`newinstance`、mount namespace 隔离与完整 devpts mount options；
- Linux 内部 `tty_driver`、flip buffer、ldisc worker、锁序、引用模型或 index allocator 的逐层仿真；
- 精确 Linux index allocator/reuse race、对 retired cached dentry 的立即物理消失保证；编号是否复用由
  identity-safety、VFS freshness 与资源耗尽 target 共同决定，不在 positioning 中预选；
- packet mode、remote mode、额外 line disciplines、virtual console 或 PTY 对 physical UART hangup 的统一抽象；
- `TIOCPKT`、`FIONREAD/TIOCINQ`、`TIOCOUTQ`、`TIOCGPTLCK` 等尚未被目标 workload 证明必要的扩展 ioctl；
- 完整 Linux termios/ioctl errno corner、完整 `VMIN/VTIME` 组合或与 ssh/tmux 无关的历史兼容面；
- ssh transport、authentication、crypto、network stack 或 tmux 自身功能；它们只作为 PTY 能力的真实 consumer；
- 除 RFC 最终接纳的 PTY master-hangup effect 外的完整 relation-disassociation signal、newly orphaned
  stopped-group 与其它 orphaned-pgrp policy；这些现有 residual 不因首版候选顺带关闭。

扩展项只有在PTY test app、sshd、tmux、LTP得分收益或新的真实程序trace证明必要时，才进入RFC review、明确的
follow-up 或 target revision，不能由“Linux 有这个 ioctl”自动晋级。

## Acceptance 方向

本 positioning 只冻结能力级验收方向，不冻结测试命令、artifact identity 或 stage：

- 仓库内普通PTY test app覆盖allocation/lock/metadata、双向I/O、termios/winsize、
  blocking/nonblocking/readiness、multiple open、dup/fork/final close、peer reopen、pathname/`TIOCGPTPEER`
  admission 等价、open-vs-retire race、stale inode fail-closed、hangup 与
  job-control；
- LTP `pty01`、`ioctl01`，以及按实际语义选取的 `ptem01`、`hangup01`；额外 line-discipline/virtual-console cases
  不自动成为首版候选 acceptance requirement；
- 固定 target artifact 的 sshd 交互 session 和 tmux create/attach/detach/exit；若其非 PTY 依赖尚未具备，必须
  记录为 Not Run，不能用 allocation smoke test 替代；
- RV64 与 LA64 的 build/runtime claim 分开记录；单架构结果不得外推。

## 晋级时识别的 Draft review questions

pre-RFC 阶段曾把以下七项统一记录为“RFC 接受前必须闭合”。public Draft 已将它们重新分类为 policy/ABI decision
与 guarantee/protocol/acceptance boundary；具体 allocator、internal callback/API 和 artifact identity 不再被误写为
同级维护者决策。以下保留原始问题集合，当前分类与权威措辞见
[RFC R1 Accepted Target](../index.md)：

- slave uid/gid/mode 的来源、permission enforcement、allocator credentials 与不建立 kernel-side grant transition 的
  可观察结果；pathname
  open 与 `TIOCGPTPEER` 必须共享该 policy，但具体 profile 尚未形成共识；
- slave pathname open 是否支持 implicit controlling-terminal acquisition，以及 `O_NOCTTY` 的对应可见效果；master
  不成为 controlling terminal 与显式 `TIOCSCTTY` 路线已经确定；
- 编号不复用造成的 boot-lifetime 累计耗尽，与安全复用所需 VFS freshness/incarnation 工作之间的 target 取舍；
- master/slave peer absence 与 hangup 的 buffered-data/read/write/ioctl/poll/epoll/errno matrix；
- master-hangup relation teardown 的 signal recipient、signal set、ordering、job-control effect，以及它相对现有
  relation-disassociation limitation 与 TTY contract 的真实 Contract Impact；
- `OPENED-DESC-003` 的单 static hook 如何与 PTY participation final release、fanotify close effect 组合，并保持
  exactly-once、既有 flock-before-hook 顺序、failure/cleanup 与 owner-private surface；
- PTY test app、sshd与tmux的输入能力、配置、依赖前提和可执行acceptance workload。

这些问题可以在 public RFC Draft review 中闭合；在闭合前不得把候选 profile、allocator 或 Linux reference result
写成 Accepted Target。归档中的“必须闭合”不表示创建 Draft 前必须已有答案。

## 暂置到 RFC 的问题

以下内容尚未在本文决定，也不应被实现者从 positioning 措辞中推断：

- semantic endpoint、PTY pair、devpts 与 relation owner 的具体类型、模块和 public/private API；
- Terminal input/output attachment 的调用方向、同步/异步选择、buffering、backpressure 与锁序；
- runtime relation enrollment/retirement 的数据结构、generation 与 cleanup transaction；
- devpts mount/publication、lookup/readdir、logical retire 与 VFS dentry cache 的具体 handoff；
- 已接受编号策略下的 allocator 表示、Kconfig 上限、live-pair accounting、inode/device-number projection 与失败
  errno 细表；
- 已接受 ABI/permission envelope 下的 `TIOCGPTPEER` flag admission、codec 与其它 workload-driven ioctl；
- Contract Impact、Implementation Boundary、target invariants、proof obligations、stage/checkpoint、probe、验证命令和
  cutover 安排。

## 归档边界

本 positioning 已于 2026-08-10 提炼为 public Draft，并因 owner/lifecycle proof 的真实需要增加
`invariants.md`。本轮没有创建 `implementation.md`、stage/checkpoint、tracking page 或 transaction。

RFC Draft 或 Accepted Target 都不授权实现；后续 target、review decision 与 Contract Impact 只在 RFC canonical
正文维护，不回写本归档。
