# PTY / devpts 实施路线

**状态：** Draft
**最后更新：** 2026-08-11
**父 RFC：** [RFC-20260810-pty-devpts](./index.md)
**当前修订：** R1
**实现授权：** None
**Contract Cutover：** None

本页只组织父 RFC R1 Accepted Target 的实施依赖、Stage 停止点与证据路线，不重新定义 target、owner、ABI、
Contract Impact 或 acceptance。Stage 1 的 Implementation Boundary 与两个 execution checkpoint 已解析，仍未获任何
执行授权；Stage 2--4 保持 Future。维护者后续可以一次授权整个 Stage 1，也可以只授权其中一个 checkpoint；只授权
Checkpoint 1 时，完成其验证与 review 后必须停止。

## 全局 Implementation Boundary

- **Target / non-goals：** 交付父 RFC 定义的单实例 Unix98 PTY / devpts、dynamic slave semantic endpoint、
  safe-reuse identity、opened-description final release、Linux-compatible open/hangup surface 与 claim-scoped
  acceptance；multiple devpts instances、legacy BSD PTY、额外 line discipline/PTY ioctl、完整 job-control residual、
  generic VFS pathname freshness 和 sshd prerequisite 均保持非目标。
- **Owner / handoff / failure / cleanup：** `Terminal`继续唯一拥有termios、winsize、line discipline与slave stream；
  PTY pair拥有identity、master liveness、slave admission/participation、peer absence与retirement；devpts拥有index与
  live binding；VFS拥有resident inode/dentry/mount projection；`task::files`拥有opened-description publication与
  final release；TTY relation、task topology、Signal和ThreadGroup job control继续各自拥有relation、membership、
  signal occurrence与stop/continue truth。跨owner effect必须先提交owner-local不可逆状态，再在guard外通过窄、
  幂等handoff推进，普通失败不得要求恢复已经退休的pair或已经发布的relation effect。
- **Protected ABI / contract / acceptance：** `PTY-DEVPTS-CUTOVER`前，current contract、existing serial ABI、
  physical UART/console owner、`OPENED-DESC-001/002/003`、VFS inode/dentry/cache owner与当前`O_NOCTTY` baseline
  均保持有效。不得提前发布半套`/dev/ptmx`/devpts ABI、success stub、第二份readiness/liveness/refcount truth，或
  只凭allocation smoke降低mandatory PTY test app的行为覆盖。
- **Validation claim：** source/owner/bypass proof、Linux 6.6.32 observable matrix、owner-local测试、仓库自有
  PTY Rust test app、选定LTP与建议性tmux共同证明各自claim；RV64/LA64 build与runtime必须分开记录，
  generic cached-positive/late-materialization保持VFS register中的Not Proven，sshd只作条件性诊断。
- **Stop conditions：** 需要改变target/non-goals、owner/handoff/failure/cleanup、public ABI、Contract Impact、
  acceptance或validation claim；无法在current single static final-release hook内自然组合PTY与fanotify effect；
  需要移动`Terminal` stream truth、建立devpts-local dentry freshness、以monotonic exhaustion规避safe reuse、
  顺带启用非目标job-control residual，或只能通过伪造环境/削弱test app覆盖运行mandatory core acceptance。

预计涉及`device::tty`、new PTY/devpts owner、VFS open/mount窄handoff、`task::files`既有static hook consumer、
`anemone-abi`、kernel Kconfig、普通PTY test app与实际system-devfs mount consumer。这些只是非穷举提示，不是逐文件
write set；同owner内部类型、模块、算法与行为保持型拆分由对应Stage实现自然闭合。

本轮实现选择仓库内普通Rust test app作为PTY用户态验证载体，形状与`socket-test`相同：`no_std` / `no_main`，使用
`anemone-rs`启动与syscall/ioctl wrapper，并作为长期guest-local test app保留。它不链接libc，也不是独立probe或
第二套semantic authority；`anemone-rs`缺少的窄ABI常量与wrapper由实际测试需求自然补齐。各Stage只规定
关闭时必须取得的行为覆盖与证据，不规定测试代码和production实现的编写先后。

## Stage 路线图

| Stage | 解析程度 | 目的 | 可见语义 / Cutover |
| --- | --- | --- | --- |
| Stage 1 | Resolved / Awaiting Authorization | 把serial-bound endpoint/relation收成runtime semantic endpoint substrate | 保持current serial行为；None |
| Stage 2 | Future | 闭合未发布的PTY pair、双向data plane、readiness与description lifecycle | 不发布PTY namespace；None |
| Stage 3 | Future | 闭合system devpts、allocation/admission、两条slave-open route与cleanup handoff | 保持PTY ABI不可发现；None |
| Stage 4 | Future | 公开激活、完成mandatory acceptance并原子cut over | `PTY-DEVPTS-CUTOVER` |

普通commit不自动形成新Stage。只有独立安全的review/授权停止点、probe、不安全中间态或contract cutover才调整
Stage路线；若后续证据要求重新解析target/owner/ABI/acceptance，则进入RFC review或Target Renegotiation，不能由
实现者自行弱化R1。

## 全局实现输入

### Live seam

| Seam | 当前事实 | 后续路线约束 |
| --- | --- | --- |
| TTY semantic/data plane | `TtyEndpoint`直接组合physical `TtyPort`、shared `Terminal`与worker wake；worker直接在port RX/TX和Terminal queue之间搬运 | Stage 1先在TTY owner内分开semantic endpoint与physical attachment participation；PTY不得实现假`TtyPort`或建立第二个`Terminal` |
| Controlling relation | relation registry在boot publication前按serial endpoint预建固定slot，并以endpoint `Arc`作为terminal identity | Stage 1提供runtime endpoint enrollment/retirement与stable identity，同时保持relation generation、topology revalidation和guards-out effect owner不变 |
| Opened description | `task::files`以published slot refcount决定final release，固定先执行flock retirement，再运行creation-time单`FileDescOps::final_release`；ordinary pathname open已把该hook用于fanotify close | PTY open owner只能在创建时把pair participation/retirement与既有fanotify effect静态组合；不得增加observer registry、多个task-owned hook或改变flock-before-hook顺序 |
| Pathname open与`O_NOCTTY` | `openat`在VFS DAC后进入`vfs_open_description()`，当前把`O_NOCTTY`记录为compat no-op而不向backend传递operation-local effect | Stage 3只为PTY slave-open success episode携带typed operation-local `O_NOCTTY`；generic serial open与opened-description status truth不得被顺带改变 |
| `TIOCGPTPEER` fd publication | ioctl FileOps当前只接收target access、userspace与arg-fd lookup；`Task::reserve_fd()` / `FdReservation::commit()`已经提供fallible reservation与infallible visibility tail | peer-open route在任何pair enrollment/relation effect前完成fd与opened-description的fallible prepare，随后只允许pair commit、conditional relation commit与fd commit组成不失败success tail；不得在FileOps内先enroll再调用可失败的普通`open_fd()` |
| Namespace与mount | devfs是persistent append-only hierarchy并已支持static directory；VFS已有no-device pseudo-filesystem与mount owner；实际init profiles在挂载devfs后建立`/dev`子mount | devfs只发布static `/dev/ptmx`和`/dev/pts` mountpoint；dynamic `N -> pair`由single-instance devpts backend拥有。Stage 3集成前必须枚举真实system-devfs mount consumers并确认同一devpts instance的mount时序，不能把dynamic namespace塞进devfs |
| ABI与capacity | `anemone-abi::tty::linux`尚无R1 PTY ioctl codec；现有Kconfig pipeline已拥有TTY/Unix等重要容量参数 | Stage 3在唯一ABI owner加入asm-generic PTY codec，并由kernel Kconfig拥有current reserved/live PTY capacity；具体默认值必须由resource/acceptance证据决定，不凭偏好预先冻结 |

这些约束只冻结自然handoff与禁止形状，不冻结Rust trait、struct、锁、container、buffer placement、模块布局或
allocator算法。进入实际Stage时若live source已经变化，应重新核验同一语义seam；只要owner与protected boundary
不变，路径变化属于route correction，不升级RFC修订。

### Source 与行为覆盖矩阵

实现与验证自然形成的逐格matrix必须能够从public source、test app与Git/PR恢复，至少覆盖：

- **Allocation / discovery：** `ptmx` allocation、initial lock、`TIOCGPTN`、`TIOCSPTLCK`、`TIOCGPTPEER` flags、
  从`TIOCGPTN`构造的slave pathname、initial metadata、valid/invalid master/fd与quota/backing-allocation errno。
- **Admission / publication：** pathname search + inode DAC与master-capability peer route的不同前置条件；
  locked/unlocked、read/write access、`O_CLOEXEC`、`O_NONBLOCK`、operation-local `O_NOCTTY`、failed-open cleanup及
  open-vs-retire的合法先后。
- **Stream / peer state：** master/slave两方向的buffered/unbuffered read/write、blocking/nonblocking、partial
  progress、last-slave-close/reopen、master retirement、EOF/`EIO`/write errno、query/mutating ioctl与
  poll/select/epoll bits。每格必须区分durable predicate和notification，不用一次wake推断结果。
- **Relation / hangup：** implicit acquisition资格矩阵、explicit `TIOCSCTTY`回归、foreground/background access、
  terminal signals、master-hangup先撤销relation可发现性再向旧session leader thread group提交`SIGHUP`、`SIGCONT`，
  以及excluded residual不被意外启用。
- **Identity / resource：** failure rollback、capacity exhaustion、retire/reuse、旧inode/open capability和迟到operation
  fail-close、current binding取得new pair；generic cached-positive、late materialization和multi-view pathname
  linearizability继续明确标为Not Proven。

Linux-visible参考以父RFC列出的tracked Linux 6.6.32 locators为主。实现中若遇到源码不能唯一确定且确实影响R1 target的
observable corner，可以按需做定向reference comparison并记录实际环境与结果；此类comparison只是执行证据，不形成
独立harness或gate，也不能覆盖fixed source或Accepted Target。

PTY test app必须检查真实能力和显式输入，并能区分allocation-only与完整语义，不能锁死developer-private artifact
provenance。长期owner-local KUnit放在被测语义文件末尾的inline `kunits`，跨模块composition测试放在最低共同owner；
不得为测试建立无production consumer的standalone probe facade。精确matrix优先由test/source保存；只有内容过长且无法
从Git/PR低成本恢复时，才在现有`backgrounds/`增加单一事实证据页，不创建通用`feedback.md`、`probe.md`或transaction。

## Stage 1 — Runtime semantic endpoint substrate

**解析状态：** Resolved / Awaiting Authorization

**Execution Authorization：** None

**Contract Cutover：** None

**Purpose：** 在TTY owner内把当前serial-bound endpoint/worker/relation shape收成可由physical serial与runtime PTY
slave共同消费的semantic endpoint capability，支持runtime relation enrollment/retirement，同时保持现有serial data
plane、boot publication、console handoff与job-control行为。

**Prerequisites：** 父RFC R1保持Accepted；Serial TTY、opened-description、iomux/epoll、device-number与file-kind
current contract保持Active；VFS dynamic positive-dentry issue保持Open / Deferred；上述live seam仍可成立；维护者另行
授权Stage 1。

### Stage 1 Implementation Boundary

- **Target / non-goals：** 在`device::tty`内关闭两个内部能力：其一是把semantic endpoint与physical serial attachment
  分开，让serial production path通过同一semantic capability工作；其二是让existing relation owner支持runtime
  terminal participant enrollment与exact retirement。Stage 1不建立PTY pair/master FileOps、peer state、hangup、devpts、
  `ptmx`、opened-description effect、implicit acquisition、`O_NOCTTY` handoff或任何用户可发现的PTY surface。
- **Owner / handoff：** semantic endpoint是stable terminal identity、shared `Terminal`与窄attachment operation
  capability的组合边界，不成为`Terminal`、transport或relation之外的新mutable owner。physical driver/`TtyPort`继续
  拥有raw RX、TX serialization与hardware-idle truth；attachment/worker只在port与`Terminal`之间搬运并请求predicate
  recheck。relation registry继续唯一拥有terminal participant membership、session binding、foreground selector与
  generation；task topology、Signal与ThreadGroup job control保持原owner。
- **Failure / cleanup：** semantic endpoint、attachment、worker与relation enrollment的全部fallible prepare发生在对应
  endpoint/file/provider visibility之前。失败只撤销本次unpublished capability；cleanup先移除registry/publication
  participation、停止外部推进，再在owner guard外drop或join。runtime relation retirement必须exact、幂等且单调：先让
  endpoint不能再被relation lookup/acquire取得并使旧snapshot失效，再释放guard；不得恢复retired identity或误删新
  enrollment。本Stage不从retirement生成`SIGHUP`/`SIGCONT`等外部effect。
- **Protected API / ABI / contract：** 所有新surface保持`device::tty` owner-local且按真实consumer给最窄visibility；
  existing serial `/dev/ttyS<N>`、`/dev/tty`、boot fd 0/1/2、termios/winsize/readiness/job-control行为、console owner、
  `TTY-PORT-001`、`TTY-ENDPOINT-001`、全部Active TTY contract与current `OPENED-DESC-*`保持不变。Stage 1没有
  `Contract Impact`或cutover，也不修改`anemone-abi`、VFS、task/Signal/job-control public/shared contract。
- **Acceptance / validation claim：** owner-local测试与source audit证明semantic endpoint不持physical port truth、
  notification不驱动行为、runtime enrollment/retirement无duplicate/stale resurrection且guard外cleanup；existing RV64
  TTY wrapper完成RV64 build/runtime并证明serial data plane、boot publication、`/dev/tty` relation、BusyBox vi/ash与正常
  关机没有回归。Stage 1不验证LA64 build/runtime；PTY test app、LTP、tmux与sshd也不属于本Stage closure，均明确记为
  Not Run且不得从RV64结果外推。
- **Stop conditions：** 必须把`Terminal` stream/readiness truth移入attachment或pair、让semantic endpoint保存第二份
  liveness/readiness、用port id/devnum/raw pointer/可升级weak reference单独决定terminal identity/live、扩大
  relation/task/Signal/job-control shared surface、改变serial visible semantics/errno、引入无当前production consumer且
  无明确Stage 2义务的facade，或需要任何PTY ABI/partial publication才能关闭本Stage。

预计实现自然涉及`device::tty`的endpoint、attachment/worker、file与relation owner，以及physical serial adapter的窄
调用面；这些是非穷举提示，不是逐文件write set。行为保持型同owner拆分、inline owner-local KUnit与必要import/re-export
可自然闭合，但不得借Stage 1重排无关TTY ABI或清理相邻代码。

### Resolved route 与不变量

1. **Semantic endpoint与attachment分离。** 当前`TtyEndpoint`直接持有`TtyPort`的形状必须消失。semantic endpoint只
   组合immutable exact terminal identity、一个shared `Terminal`和打开/推进operation所需的窄attachment capability；
   physical serial attachment/worker持有`TtyPort`并消费该endpoint。`TtyFile`、relation与`/dev/tty`不得依赖physical
   port private representation。endpoint的初始line snapshot/profile改由owner-neutral输入形成：serial仍提交真实
   boot-applied line snapshot，未来PTY可以提交logical profile；committed termios、winsize与stream truth仍只在
   `Terminal`。
2. **Progress capability不是状态owner。** opened file可以持有保证本次operation推进所需的strong capability，semantic
   endpoint只保留不制造reference cycle的open/upgrade边界；具体strong/weak表示由实现选择。wake只请求worker/peer重验
   `TtyPort`、`Terminal`或未来pair的durable predicate，不携带byte、count、readable/writable/drain或liveness truth。
   attachment/backend capability不可取得时，新的open或operation prepare必须fail closed；already-open operation的
   terminal outcome仍由对应owner predicate与后续Stage的hangup协议决定，不能从`Weak::upgrade()`结果反推pair/terminal
   lifecycle。
3. **Relation enrollment只有一个truth。** endpoint creation/attachment prepare至多产生一次owner-local enrollment
   authority；relation registry成功commit后才是participant registration/currentness的唯一真相。serial endpoint必须在
   现有boot devfs/file publication前完成enrollment，pre-publication abort必须撤销自己；boot naming、selected console
   identity、devnum与publish顺序不因registry变为runtime shape而改变。endpoint自身不得缓存`registered` bool，relation
   也不得用serial port registry、published endpoint vector或devfs node反推membership。
4. **Identity与retirement。** relation key必须绑定immutable semantic-terminal identity；`TtyPortId`、`CharDevNum`、
   vector index、裸地址或可升级weak handle均不能单独成为identity。duplicate enrollment在visibility前稳定失败；retire
   只命中自己的exact enrollment，使旧relation snapshot/generation不能commit，并且不影响concurrent/new endpoint。
   serial normal path仍publish-until-reboot；runtime retirement capability在Stage 1只关闭relation owner-local
   discoverability，不提前实现pair retirement、hangup predicate或Signal effect。
5. **锁与cleanup顺序。** relation guard内只检查/提交participant、binding、foreground与generation；不得进入Terminal、
   port、task topology、Signal、Event、worker join或复杂drop。notification、join和外部owner drop均在guard外执行。
   cleanup/`Drop`路径先撤销仍由本owner持有的publication/enrollment，再用常开assertion暴露duplicate/lost cleanup；
   notification count、diagnostic id和日志不得参与状态转换。

### Checkpoint 1 — Semantic endpoint / serial attachment closure

**Purpose：** 只关闭semantic endpoint与physical serial attachment的生产路径分离；relation仍可保持现有boot-prepared
participant shape。该中间态独立安全、由existing serial endpoint全部消费，并为relation lifecycle变化提供单独review点。

**Deliverable：**

- semantic endpoint不再持有`TtyPort`，physical attachment/worker持port并通过窄capability消费shared `Terminal`；
- line profile、opened file progress capability与`/dev/tty` open route不再泄漏physical-port representation；
- serial RX/TX、drain、control-character signal、winsize signal、boot publication、console handoff和pre-publication abort
  全部迁移到新形状，不保留old/new双路径、temporary adapter或仅供测试的facade；
- 以常开assertion和inline KUnit覆盖duplicate attach、worker spawn failure、abort顺序、wake只促使predicate recheck、
  reference-cycle absence与selected endpoint仍使用同一Terminal。

**Validation：**

1. `git diff --check`与`just fmt kernel --check`；
2. `./scripts/run-tty-test-rv64.sh --busybox <rv64-busybox> --sdcard <rv64-sdcard-master> --mode auto --log
   build/pty-devpts-stage1-ckpt1-rv64.log`，由wrapper完成tracked rootfs、RV64 build、KUnit、TTY auto/vi/ash oracle与正常
   关机；
3. source/bypass audit确认physical port只由driver/attachment/worker持有，FileOps/relation只消费semantic capability，
   current serial ABI、devnum、boot fd与console owner没有旁路或第二份truth。

**Cutover / Exit：** None。所有existing serial production caller都使用新形状、上述验证通过且review无未关闭的
Apollyon/Keter后，Checkpoint 1可关闭；这不表示Stage 1或任何PTY能力完成。若只授权Checkpoint 1，立即停止，不进入
Checkpoint 2。

### Checkpoint 2 — Runtime relation participant lifecycle

**Prerequisite：** Checkpoint 1已关闭；semantic endpoint/current serial path保持稳定；维护者已授权整个Stage 1或另行授权
Checkpoint 2。

**Purpose：** 把boot-fixed relation slots替换为runtime semantic-terminal enrollment/retirement，同时让serial boot path
成为第一个production consumer并保持全部current relation/job-control语义。

**Deliverable：**

- relation owner提供fallible pre-visibility enrollment、exact duplicate rejection、generation-scoped lookup/mutation与
  explicit idempotent retirement；registration authority只表达提交能力，不复制registry liveness truth；
- serial attachment/boot transaction完成真实enroll与失败rollback，`/dev/ttyS<N>`、boot files和`/dev/tty`只在完整
  endpoint/relation capability可用后发布；serial成功publication后仍保持到reboot；
- retirement先撤销endpoint与旧relation的discoverability并使stale snapshot不能commit，再在guard外drop/wake；本
  checkpoint不产生hangup signal、不修改existing session-exit/`TIOCNOTTY` effect，也不为future PTY保存pair state；
- inline relation-owner KUnit或最低共同owner composition测试覆盖duplicate enrollment、failed prepare、retire-no-entry、
  retire-live-entry、stale generation、old-identity suppression、new endpoint不被旧cleanup命中和guards-out drop。测试必须
  走production registry transition，不增加validation-only facade。

**Validation：**

1. `git diff --check`与`just fmt kernel --check`；
2. `./scripts/run-tty-test-rv64.sh --busybox <rv64-busybox> --sdcard <rv64-sdcard-master> --mode auto --log
   build/pty-devpts-stage1-ckpt2-rv64.log`，要求全部KUnit、TTY relation/data-plane/vi/ash oracle通过且正常关机；
3. source/lock/bypass audit证明participant membership与session relation只有registry一份truth，endpoint/Session/Terminal/
   port/published vectors均无mirror，relation guard内没有task/Signal/Event/worker/drop调用，retired/stale identity不能重新
   取得或提交relation。

**Cutover / Exit：** None。两项Stage target、validation与Architecture Friction Scan均关闭后，Stage 1记为Closed并立即
停止；Stage 2仍为Future且未获解析或执行授权。LA64 build/runtime、PTY app、LTP、tmux、sshd保持Not Run。若runtime
registry只能靠generic lifecycle observer、第二份endpoint liveness、pair/PTY-specific branch、task/Signal owner改动或
serial ABI变化才能成立，必须在Stage 1完成声明前停止并回到RFC review / Target Renegotiation。

## Stage 2 — PTY pair、data plane 与 opened-description lifecycle

**Purpose：** 在未发布namespace内闭合PTY pair identity、master/slave FileOps、双向Terminal data plane、peer
presence/absence、readiness、hangup/retirement与master/slave opened-description final-release participation，形成可由
Stage 3 allocation/open transaction消费的完整owner-private capability。

**Prerequisites：** Stage 1关闭；runtime semantic endpoint与relation enrollment substrate已证明保持serial行为；
维护者另行授权Stage 2。

**Protected Boundary：** pair不取得Terminal/relation/fd-table truth，FileOps不缓存第三份readiness/HUP状态，master不加入
controlling relation，final release不依赖`Arc`/fd/inode引用，Stage 2不注册devpts或发布任何部分PTY ABI。到达Stage 2前
不冻结buffer placement、worker模型、pair container或内部callback形状。

## Stage 3 — System devpts、allocation/admission 与跨 owner cleanup

**Purpose：** 组合single-instance devpts index/binding与VFS projection，闭合prepare-before-publish allocation、Kconfig
capacity/safe reuse、initial metadata、pathname与`TIOCGPTPEER`两条route、PTY ioctl、implicit acquisition success tail、
static final-release composition和master-retirement的devpts/relation/Signal/waiter handoff；完成但暂不公开激活R1 surface。

**Prerequisites：** Stage 2关闭；pair capability、observable matrix、opened-description effect与system-devfs mount consumer
均已可供集成；维护者另行授权Stage 3。

**Protected Boundary：** devpts不读取VFS private dentry/cache，不以numeric index替代episode identity，不改变
`OPENED-DESC-003`或generic serial `O_NOCTTY`语义，不启用excluded job-control residual，不注册可由普通用户发现的半套
PTY surface。到达Stage 3前不冻结allocator、incarnation、inode/container或mount-integration的具体表示。

## Stage 4 — Public activation、acceptance 与 `PTY-DEVPTS-CUTOVER`

**Purpose：** 原子公开static `/dev/ptmx`、system `/dev/pts` view与完整R1 operation surface，运行mandatory
source/owner、PTY Rust test app、LTP和architecture evidence，尝试并归因tmux，随后一次性完成父RFC列出的
PTY/DEVPTS/TTY contract Introduce/Refine并关闭RFC；sshd仅在条件具备时作为诊断consumer。

**Prerequisites：** Stage 3关闭；全部production path、rollback/cleanup和acceptance consumer已在同一可review状态；
维护者另行授权Stage 4与`PTY-DEVPTS-CUTOVER`。

**Protected Boundary：** core claim缺失时不得部分cut over或用tmux/sshd环境结果改写target；RV64/LA64 build/runtime分别
记录，只有实际运行的架构取得runtime-proven claim；generic VFS pathname freshness继续留在register，current limitation
只按真实LTP/ABI证据缩减。到达Stage 4前不冻结artifact identity、精确命令或未要求的额外runtime范围。

## 证据与反馈路由

- target、owner、ABI、Contract Impact与acceptance变化只写父RFC并经review；implementation route correction写本页。
- 每个Stage的实际代码、测试、命令与结果优先由Git/PR保存；只有长期多Stage执行历史、probe或renegotiation确有需要时
  才创建transaction。
- `PTY-DEVPTS-CUTOVER`前不更新current contract；cutover时只更新父RFC列出的真实Introduce/Refine ID。
- 新发现的当前缺陷或接受限制进入register；VFS dynamic positive-dentry issue不因PTY source/test通过而关闭。
- 每个Stage收口前执行Architecture Friction Scan；只有具体Euclid/Keter/Apollyon证据才写回本页或RFC closure，Safe不
  留占位结论。
