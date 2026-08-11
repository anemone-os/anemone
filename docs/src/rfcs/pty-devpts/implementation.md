# PTY / devpts 实施路线

**状态：** Draft
**最后更新：** 2026-08-11
**父 RFC：** [RFC-20260810-pty-devpts](./index.md)
**当前修订：** R3
**Stage 状态：** Stage 1--3 / Closed；Stage 4 / Future
**Execution Authorization：** Stage 1--3已消费并关闭；Stage 4：None
**Contract Cutover：** None

本页只组织父 RFC R3 Accepted Target 的实施依赖、Stage 停止点与证据路线，不重新定义 target、owner、ABI、
Contract Impact 或 acceptance。Stage 1 的 Implementation Boundary 与两个 execution checkpoint 已解析并在各自授权下
关闭。Stage 2与Stage 3的两个checkpoint也已在各自独立授权下完成并关闭，均未产生contract cutover；当前执行严格停在
Stage 3 closure。既有路线或Stage 3 handoff不构成Stage 4 execution authorization；Stage 4保持Future。

## 全局 Implementation Boundary

- **Target / non-goals：** 交付父 RFC 定义的user-mountable single-persistent-instance Unix98 PTY / devpts、dynamic slave semantic endpoint、
  safe-reuse identity、opened-description final release、Linux-compatible open/hangup surface 与 claim-scoped
  acceptance；additional mount只形成同一instance的VFS view；multiple/private devpts instances、mount-local `ptmx`、
  legacy BSD PTY、额外line discipline/PTY ioctl、完整job-control residual、generic VFS pathname freshness和sshd
  prerequisite均保持非目标。
- **Owner / handoff / failure / cleanup：** `Terminal`继续唯一拥有termios、winsize、line discipline与slave stream；
  PTY pair拥有identity、master liveness、slave admission/participation、peer absence与retirement；devpts拥有persistent
  system instance、index与live binding；VFS拥有singleton superblock/inode/dentry与distinct mount-view projection；
  `task::files`拥有opened-description publication与
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
  需要per-mount backend/binding、按mount path选择instance、让mount count驱动pair lifecycle、顺带启用非目标job-control
  residual，或只能通过伪造环境/削弱test app覆盖运行mandatory core acceptance。

预计涉及`device::tty`、new PTY/devpts owner、VFS open/mount窄handoff、devfs static mountpoint、persistent init consumers、
`task::files`既有static hook consumer、
`anemone-abi`、kernel Kconfig、普通PTY test app与实际system-devfs mount consumer。这些只是非穷举提示，不是逐文件
write set；同owner内部类型、模块、算法与行为保持型拆分由对应Stage实现自然闭合。

本轮实现选择仓库内普通Rust test app作为PTY用户态验证载体，形状与`socket-test`相同：`no_std` / `no_main`，使用
`anemone-rs`启动与syscall/ioctl wrapper，并作为长期guest-local test app保留。它不链接libc，也不是独立probe或
第二套semantic authority；`anemone-rs`缺少的窄ABI常量与wrapper由实际测试需求自然补齐。各Stage只规定
关闭时必须取得的行为覆盖与证据，不规定测试代码和production实现的编写先后。

## Stage 路线图

| Stage | 解析程度 | 目的 | 可见语义 / Cutover |
| --- | --- | --- | --- |
| Stage 1 | Closed | 把serial-bound endpoint/relation收成runtime semantic endpoint substrate | 保持current serial行为；None |
| Stage 2 | Closed | 闭合未发布的PTY pair、双向data plane、readiness与description lifecycle | 不发布PTY namespace；None |
| Stage 3 | Closed | 闭合system devpts、allocation/admission、两条slave-open route与cleanup handoff | 保持PTY ABI不可发现；None |
| Stage 4 | Future | 公开激活、完成mandatory acceptance并原子cut over | `PTY-DEVPTS-CUTOVER` |

普通commit不自动形成新Stage。只有独立安全的review/授权停止点、probe、不安全中间态或contract cutover才调整
Stage路线；若后续证据要求重新解析target/owner/ABI/acceptance，则进入RFC review或Target Renegotiation，不能由
实现者自行弱化R3。

## 全局实现输入

### Live seam

| Seam | 当前事实 | 后续路线约束 |
| --- | --- | --- |
| TTY semantic/data plane | `TtyEndpoint`只组合stable identity、shared `Terminal`与weak progress source；physical serial attachment/worker独立持有`TtyPort`，Stage 2 PTY pair通过同一`Terminal`与progress capability闭合双向data plane | Stage 3保持`Terminal`、physical port与pair truth边界，不恢复serial-bound endpoint、不实现假`TtyPort`或建立第二个`Terminal` |
| Controlling relation | relation registry已支持runtime exact enrollment/retirement与participant/relation双generation重验；serial participant在boot publication前commit并保留到reboot，Stage 2 pair仍持uncommitted enrollment authority | Stage 3 allocation在pair/binding/fd visibility前fallibly commit exact participant；retirement只通过该participant与旧relation generation形成guards-out cleanup effect，不让pair复制relation truth |
| Opened description | `task::files`以published slot refcount决定final release，固定先执行flock retirement，再运行creation-time单`FileDescOps::final_release`；Stage 2 PTY master/slave effect已在该单hook内转发既有base effect | Stage 3只在创建时静态补齐devpts/relation/Signal cleanup capability并保留fanotify base effect；不得增加observer registry、多个task-owned hook或改变flock-before-hook顺序 |
| Pathname open与`O_NOCTTY` | `openat`在VFS DAC后进入`vfs_open_description()`，当前把`O_NOCTTY`记录为compat no-op而不向backend传递operation-local effect | Stage 3只为PTY slave-open success episode携带typed operation-local `O_NOCTTY`；generic serial open与opened-description status truth不得被顺带改变 |
| `TIOCGPTPEER` fd publication | ioctl FileOps当前只接收target access、userspace与arg-fd lookup；`Task::reserve_fd()` / `FdReservation::commit()`已经提供fallible reservation与infallible visibility tail | peer-open route在任何pair enrollment/relation effect前完成fd与opened-description的fallible prepare，随后只允许pair commit、conditional relation commit与fd commit组成不失败success tail；不得在FileOps内先enroll再调用可失败的普通`open_fd()` |
| Namespace与mount | devfs是persistent append-only hierarchy、支持static directory且拒绝userspace `mkdir`；VFS允许filesystem mount callback复用同一superblock，并以`PERSISTENT_SB`保留last-unmount lifetime；generic `mount(2)`已有`CAP_SYS_ADMIN` admission | Stage 3形成mount-ready single persistent instance，empty data的任意target mount都返回同一superblock/root；Stage 4由devfs发布static `/dev/ptmx`与`/dev/pts` mountpoint并让persistent init显式mount。additional view不创建instance，temporary devfs不自动mount |
| ABI与capacity | `anemone-abi::tty::linux`尚无R3 PTY ioctl codec；现有Kconfig pipeline已拥有TTY/Unix等重要容量参数 | Stage 3在唯一ABI owner加入asm-generic PTY codec，并由kernel Kconfig拥有current reserved/live PTY capacity；具体默认值必须由resource/acceptance证据决定，不凭偏好预先冻结 |

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
  fail-close、current binding取得new pair；两个fresh mount view必须取得同一superblock/inode/binding，single-view
  unmount不影响另一view，last-view unmount/remount保留system instance；generic cached-positive、late materialization、
  readdir与retire/reuse的完整multi-view pathname linearizability继续明确标为Not Proven。

Linux-visible参考以父RFC列出的tracked Linux 6.6.32 locators为主。Linux `devpts_mount`的per-mount private instance只
是差异参考，不能覆盖R3 single-instance decision。实现中若遇到源码不能唯一确定且确实影响R3 target的
observable corner，可以按需做定向reference comparison并记录实际环境与结果；此类comparison只是执行证据，不形成
独立harness或gate，也不能覆盖fixed source或Accepted Target。

PTY test app必须检查真实能力和显式输入，并能区分allocation-only与完整语义，不能锁死developer-private artifact
provenance。长期owner-local KUnit放在被测语义文件末尾的inline `kunits`，跨模块composition测试放在最低共同owner；
不得为测试建立无production consumer的standalone probe facade。精确matrix优先由test/source保存；只有内容过长且无法
从Git/PR低成本恢复时，才在现有`backgrounds/`增加单一事实证据页，不创建通用`feedback.md`、`probe.md`或transaction。

## Stage 1 — Runtime semantic endpoint substrate

**解析状态：** Closed

**Execution Authorization：** Stage 1两个checkpoint均已消费并关闭；Stage 2：None

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

**Execution Result（2026-08-11）：** Closed。semantic endpoint、serial attachment/worker、line profile与opened-file
progress capability已按本checkpoint分离；production source/bypass audit、618项KUnit、RV64 TTY auto/vi/ash oracle、
正常关机与独立review均通过。没有contract cutover或register变化；LA64、PTY app、LTP、tmux与sshd保持Not Run。
完整执行事实与review更正见[transaction](../../devlog/transactions/2026-08-11-pty-devpts.md)。该轮在Checkpoint 1停止；
Checkpoint 2后来由维护者另行授权。

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
停止；本checkpoint关闭时Stage 2仍为Future且未获解析或执行授权，后续docs-only解析不回溯扩大该授权。LA64
build/runtime、PTY app、LTP、tmux、sshd保持Not Run。若runtime
registry只能靠generic lifecycle observer、第二份endpoint liveness、pair/PTY-specific branch、task/Signal owner改动或
serial ABI变化才能成立，必须在Stage 1完成声明前停止并回到RFC review / Target Renegotiation。

**Execution Result（2026-08-11）：** Closed。relation registry现支持pre-visibility runtime enrollment、exact/idempotent
retirement与participant/relation双generation revalidation；serial boot transaction在其它fallible prepare完成后才消费
enrollment，并把成功participant保留到reboot。duplicate、partial prepare rollback、retire-no-entry/exact cleanup与
distinct replacement endpoint stale-cleanup KUnit均通过。source/lock/bypass audit确认registry仍是membership与session
relation的唯一truth，旧snapshot不能越过endpoint identity与双generation核验，retirement先撤销discoverability再在
guard外drop。最终RV64 wrapper完成repository build，622/622 KUnit、TTY relation/data-plane/vi/ash与50/50 guest oracle
全部通过并正常关机；独立review无Apollyon/Keter。没有contract cutover或register变化；LA64 build/runtime、PTY app、
LTP、tmux与sshd保持Not Run。Stage 1关闭，执行立即停止，Stage 2仍未授权。

## Stage 2 — PTY pair、data plane 与 opened-description lifecycle

**解析状态：** Closed

**Execution Authorization：** Stage 2已消费并关闭；Stage 3：None

**Contract Cutover：** None

**Purpose：** 在未发布namespace内闭合PTY pair identity、master/slave FileOps、双向Terminal data plane、peer
presence/absence、readiness、hangup/retirement与master/slave opened-description final-release participation，形成可由
Stage 3 allocation/open transaction消费的完整owner-private capability。

**Prerequisites：** Stage 1关闭；runtime semantic endpoint与relation enrollment substrate已证明保持serial行为；
父RFC R1继续Accepted，current TTY、opened-description、iomux/epoll与VFS contracts未漂移；维护者另行明确授权Stage 2。

### Stage 2 Implementation Boundary

- **Target / non-goals：** 关闭一个route-neutral、owner-private的PTY pair capability：每个pair有不可跨episode复活的
  identity、一个带未提交one-shot relation enrollment的slave semantic endpoint、master/slave production FileOps、
  route-neutral slave-description participation、双向stream/readiness以及pair-local peer absence、hangup和retirement。
  master不是semantic TTY endpoint且不产生relation enrollment。Stage 2不建立devpts index/binding、`/dev/ptmx`或
  `/dev/pts/N`，不实现initial lock、pathname/`TIOCGPTPEER` route policy、PTY-specific ioctl、
  implicit controlling-terminal acquisition、safe-reuse allocator、metadata/DAC或任何用户可发现的PTY ABI；这些仍由
  Stage 3组合。PTY test app、LTP、tmux和sshd不能访问本Stage capability，不属于closure oracle。
- **Owner / handoff：** `Terminal`继续唯一拥有termios、winsize、line discipline、conditioned input和processed output；
  pair owner唯一拥有pair identity、master liveness、slave-description participation、peer absence与retirement。
  relation registry继续唯一拥有terminal membership与session relation；prepared pair只持Stage 1定义的pre-visibility
  enrollment authority，Stage 2不提交它或从它推断membership。
  `task::files`的published-slot count继续是opened-description final release唯一真相，只通过creation-time固定的
  `FileDescOps::final_release`向pair提交master retirement或slave participant release。master/slave FileOps只组合本次
  operation所需的Terminal/pair snapshot与progress capability，不取得relation、fd-table或future devpts truth。
- **Failure / cleanup：** pair、Terminal、semantic endpoint、FileOps private capability及description participation所需的
  fallible prepare必须发生在pair/description visibility或participation commit之前。未提交的master/slave description
  只撤销本次prepared capability；slave participation commit与master retirement在pair owner内线性化。master final
  release先不可逆地禁止新participation并发布pair-local retired/hangup predicate，再释放pair guard；Stage 2只形成供
  Stage 3消费的窄、幂等cleanup事实，不调用devpts、relation、Signal、ThreadGroup或VFS cleanup，也不通过恢复live状态
  回滚外部失败。last-slave-description release只形成可重新participate的peer absence，不退休pair。
- **Protected API / ABI / contract：** 新surface保持TTY/PTY owner-local、按真实Stage 2/3 consumer给最窄visibility；
  physical serial attachment/worker、existing `/dev/ttyS<N>`、`/dev/tty`、boot fd、console、termios/winsize/job-control、
  current `TTY-*`、`OPENED-DESC-*`与iomux/epoll contracts均保持不变。Stage 2不得修改`anemone-abi`、devfs/VFS
  namespace、generic open/`O_NOCTTY`路径或current contract，也不得把pair final-release effect实现成dynamic observer
  registry、第二个task-owned hook或`Arc`/fd/inode refcount推断。
- **Acceptance / validation claim：** 纯owner-local deterministic tests与source/lock audit证明pair identity、participation、
  peer state和retirement只有pair owner一份truth；master write进入shared Terminal input/line-discipline，slave write与echo
  进入同一Terminal output并由master消费；master/slave对既有termios/winsize profile观察同一Terminal truth，master不因此
  取得controlling-relation operation surface；blocking/nonblocking、partial progress、poll subscription/recheck与hangup
  结果使用同一durable predicates。`task::files`既有published-slot owner tests证明dup/fork、close-on-exec与table teardown的
  通用opened-description语义；PTY static final-release composition、concurrent final close、participate-vs-retire、lost-wake与
  锁顺序只由source/lock/linearization review证明，不为此新增file-table fixture或把KUnit作为并发证明。RV64 canonical TTY
  wrapper证明TTY/PTY scope剩余pure KUnit与existing serial 50/50、vi/ash、正常关机无回归；repository-wide KUnit总数只作
  build/regression事实，不外推并发证明。LA64只要求repository kernel build，不由此取得runtime-proven claim。
- **Stop conditions：** 需要移动或复制`Terminal` stream/readiness truth、以fake `TtyPort`承载PTY、用wake count或
  upgradeable weak handle决定liveness、让pair读取fd-table/relation/private VFS state、改变single-static-hook或
  flock-before-hook规则、无法在Stage 3继续owner-locally组合pair与fanotify effect、必须提前接入devpts/PTY UAPI才能证明
  core语义、需要改变Linux-default observable matrix、父RFC target/owner/ABI/Contract Impact/acceptance，或只能通过
  validation-only facade、test branch、success stub和降低race/readiness oracle完成本Stage。

预计实现自然涉及`device::tty`内的semantic endpoint、Terminal、serial file seam与new PTY pair/master/slave owner，
以及`task::files`既有static final-release consumer。这些只是非穷举提示，不是逐文件write set。
现有TTY文件已混合serial FileOps、ABI、relation与Terminal操作；若继续加入PTY职责会混淆owner，可在同一TTY owner内按
pair、file/ops或lifecycle等稳定职责做行为保持型拆分，但不得扩大public API、visibility contract或制造通用callback层。

### Resolved route 与 proof obligations

1. **Pair episode与prepared capability。** 每次构造产生新的immutable pair/slave-terminal identity；全部内部allocation
   在返回prepared pair前完成。prepared master/slave description capability只能被消费一次，abort不增加participant、
   不发布hangup或留下waiter。Stage 3可以编排该capability，但不能取得pair runtime truth或把numeric PTY index变成
   identity。Stage 2不实现index/incarnation；episode隔离由immutable allocation identity与source/lifecycle review证明。
2. **双向Terminal data plane。** master write必须通过现有Terminal RX conditioning/line discipline进入slave input；
   slave write、echo和output processing继续进入同一Terminal output，由master read消费。PTY不伪造physical port或建立
   parallel input/output queue。master/slave FileOps共享父RFC已接受的termios/winsize truth和对应ordinary operation，
   但master不暴露或转发`TIOCSCTTY`等controlling-relation operation；PTY-specific ioctl仍由Stage 3实现。backend
   progress只请求对应owner重验predicate；buffer placement、是否需要worker及具体callback/type由实现选择，但serial
   production path必须保持单一路径和现有行为。
3. **Opened-description participation。** pair owner提供route-neutral的slave participation prepare/commit/release与master
   retirement effect；Stage 3再在pathname/peer route完成lock、permission和fd publication编排。Stage 2 source review沿真实
   `FileDesc`/file-table publication与static final release追踪effect，并复用`task::files` owner已有的published-slot证据；不为
   PTY KUnit复制file-table lifecycle或直接调用private decrement冒充final-release composition证明。dup/fork aliases共享同一
   description participation；独立opens分别participate；transient syscall `Arc`、File storage、inode/dentry ref不延迟或触发
   semantic release。
4. **Peer absence与retirement。** master live时zero slave descriptions只形成peer absence，后续route-neutral participation
   可以使peer重新出现。master description final release是唯一pair retirement trigger：先禁止新participation并发布
   retired/hangup，再在guard外wake/recheck。concurrent participate-vs-retire只能得到完整participant或fail-closed，late
   participant release与重复cleanup均不能复活pair或影响distinct pair。
5. **Readiness与observable matrix。** Stage 2从父RFC全局matrix中关闭stream/peer-state子集：master/slave两方向的
   buffered/unbuffered、blocking/nonblocking、partial progress、last-slave-close/reopen、master-first/slave-first close、
   shared termios/winsize round-trip、master relation-ioctl rejection、EOF/`EIO`/write errno以及poll/select/epoll
   READABLE/WRITABLE/HUP/ERR投影。具体逐格结果按tracked Linux 6.6.32 source与定向测试形成implementation evidence；
   FileOps、blocking wait与poll route必须组合同一Terminal/pair predicates，notification只触发snapshot/register/recheck/
   final snapshot。
6. **Stage 3 handoff保持窄。** Stage 2 closure只保证prepared pair、slave endpoint的uncommitted relation enrollment、
   FileOps、description effect与pair-local retirement fact可被后续allocation/open transaction消费；不预建devpts/VFS/
   relation/Signal callback，不提交relation enrollment，也不冻结allocator、index、metadata、mount、ioctl codec或
   implicit-acquire success tail。Stage 3若不能在这些owner-private capability之上自然完成static composition，必须回到
   RFC review，不能在Stage 2预留dynamic bridge。

### Execution shape

Stage 2作为一个formal stage整体授权、review和关闭，不增加execution checkpoint或semantic cutover。实现可以按
“pair/data-plane production path -> opened-description participation -> concurrency source review / observable validation”排序
并形成普通commit，但这些顺序不建立独立Ready/Active/Closed状态，也不允许只完成前一部分就把dormant pair core声明为
Stage能力。
若实际证据表明必须设置独立probe、不安全中间态或额外授权停止点，先更新本页并review其必要性，不在实现中临时发明gate。

### Validation 与 closure

1. `git diff --check`、`just fmt kernel --check`与`mdbook build docs`；
2. source/owner/lock/bypass audit确认Terminal、pair、opened-description三份truth边界，pair guard内无fd-table、relation、
   Signal、VFS、Event notification或复杂drop，serial与PTY没有old/new双路径、fake port或缓存readiness/HUP；concurrent
   participate-vs-retire、concurrent final close、lost-wake与锁顺序只由该source/lock/linearization review证明，本Stage不以
   KUnit或其它单任务测试声称runtime interleaving proof；
3. inline owner-local KUnit只覆盖不进入scheduler、timer、sleep、wait或poll registration的纯确定性transform、buffer、
   nonblocking data-plane与snapshot semantics；不得写并发/交错KUnit，不为KUnit扩大production visibility或新增file-table/
   validation fixture，也不新建独立`kunit.rs`/`tests.rs`；PTY blocking、poll registration/final recheck、description
   publication与lifecycle composition由第2项source review承担，Stage 2不把existing serial userspace回归外推为未公开PTY的
   runtime proof；
4. canonical RV64 TTY wrapper完成repository build、repository KUnit runner、existing TTY `50/50`、BusyBox vi/ash、host
   byte oracle与正常关机；只有符合第3项边界的TTY/PTY KUnit形成Stage 2 evidence，repository总数不承担并发证明；本Stage
   不要求公开PTY namespace，因此不能以PTY userspace smoke替代owner-local/source coverage；
5. canonical LA64 repository kernel build通过；LA64 runtime、PTY test app、LTP、tmux与sshd保持Not Run，且不得从RV64
   runtime或双架构build外推；
6. stage-wide review与Architecture Friction Scan检查第二份stream/peer/refcount truth、owner穿透、static-hook扩张、
   caller/test/architecture特判、无退出条件bridge和隐含cleanup顺序；Apollyon/Keter必须在closure前消除或触发停止，
   未在边界内消除的Euclid按workflow写回。

**Cutover / Exit：** None。prepared pair capability、master/slave production FileOps、pair-local lifecycle、真实
opened-description final-release participation、stream/peer-state observable matrix、双架构build floor与RV64 serial/pure KUnit
回归全部关闭，且review/Architecture Friction Scan满足上项条件后，Stage 2才可记为Closed。关闭不发布PTY namespace/
ABI、不更新current contract或register，并立即停止；Stage 2关闭当时Stage 3仍为Future，需要新的解析与执行授权。

**Execution Result（2026-08-11）：** Closed。新增owner-private `pty`模块形成prepared/live pair、master/slave FileOps、
shared Terminal双向data plane、peer absence/reopen、master retirement、mandatory HUP/ERR与static final-release effect；
`operation`只串行化bounded Terminal commit与description release，不拥有状态真相。poll register路径先安装route且不取得
sleepable mutex；snapshot/final scan在iomux round退役后取得`operation`，避免把pre-retirement liveness与post-retirement
buffer flush拼成不存在的空readiness。

KUnit边界按维护者更正收紧：删除TTY worker/KThread/Event-timeout、PTY iomux wait-round、Terminal iomux wait-round及
跨owner file-table fixture/alias-fork-cloexec composition测试，共移除12项；remaining TTY/PTY KUnit只执行pure deterministic、
nonblocking owner-local transform/buffer/data-plane/snapshot，不进入scheduler、timer、sleep、wait或poll registration。
concurrent participate-vs-retire、final close、close-vs-poll、lost-wake与锁序只由source/lock/linearization review证明；没有
runtime interleaving proof，且未把KUnit计数写成并发证据。独立review最终为0 Apollyon / 0 Keter / 0 Euclid，Architecture
Friction Scan未发现需要保留的具体摩擦。

最终canonical RV64 wrapper以smp=1、memory=1G通过：616/616 KUnit、`TTYTEST:SUMMARY:PASS:50`、BusyBox vi/ash、
`TTY-HARNESS:PASS:auto-byte-checks`与orderly shutdown；该userspace结果只证明existing serial回归，不外推未公开PTY runtime。
canonical LA64 repository build通过，final symbol table为6255 entries。LA64 runtime、PTY test app、LTP、tmux与sshd均
Not Run。current contract、register与`PTY-DEVPTS-CUTOVER`保持不变；Stage 3仍未授权。

## Stage 3 — System devpts、allocation/admission 与跨 owner cleanup

**解析状态：** Closed

**Execution Authorization：** Checkpoint 1--2已消费并关闭

**Contract Cutover：** None

**Purpose：** 组合single-instance devpts index/binding与VFS projection，闭合prepare-before-publish allocation、Kconfig
capacity/safe reuse、initial metadata、pathname与`TIOCGPTPEER`两条route、PTY ioctl、implicit acquisition success tail、
static final-release composition和master-retirement的devpts/relation/Signal/waiter handoff；完成但暂不公开激活R3 surface。

**Prerequisites：** Stage 2关闭；pair capability、observable matrix、opened-description effect与system-devfs mount consumer
均已完成live-source核验；父RFC R3与current TTY、opened-description、VFS、iomux/epoll contracts未漂移；维护者按下述
停止点另行授权对应checkpoint。解析本节或授权Checkpoint 1都不自动授权Checkpoint 2。

### Stage 3 Implementation Boundary

- **Target / non-goals：** 先在TTY owner内完成Stage 3新增职责所必需的行为保持型模块拆分，再关闭一个仍不可由用户发现的
  production集成状态：single persistent devpts instance拥有capacity reservation、index/live binding、accepted initial
  metadata source与logical retirement，并提供empty-data mount总是返回同一prebuilt superblock/root的production route；
  `/dev/ptmx` allocation、pathname slave open与`TIOCGPTPEER` peer open都消费Stage 2的
  prepared pair/description capability；PTY ioctl、implicit controlling acquisition与master-retirement cleanup走真实owner
  handoff。Stage 3不把devpts filesystem注册进public registry，不向devfs发布`/dev/ptmx`或`/dev/pts`，不修改init/
  rootfs mount consumer，也不运行或声称PTY userspace acceptance；公开激活、system mount与mandatory app/LTP/tmux留给
  Stage 4原子完成。
- **Owner / handoff：** `Terminal`继续唯一拥有termios、winsize、line discipline、slave stream与对应readiness；PTY pair
  继续拥有episode identity、slave lock、master liveness、description participation、peer absence、hangup与retirement；new
  `fs::devpts`唯一拥有system instance、numeric index、current `N -> pair` binding、metadata input与logical binding
  retirement；VFS继续拥有singleton superblock/inode/dentry/cache、distinct mount view与DAC；`task::files`继续拥有fd
  reservation/publication和opened-description final release；TTY
  relation、task topology、Signal与ThreadGroup job control保持原owner。跨owner只传prepared capability、exact episode/
  binding capability、operation-local open effect与cleanup snapshot，不传完整`Task`、file table、VFS private cache或pair
  private guard。
- **Failure / cleanup：** allocation先完成credential snapshot、capacity/index reservation、pair/master description、
  metadata/binding projection与relation participant enrollment等全部fallible prepare，随后才进入pair commit、binding/fd
  publication组成的不失败success tail；失败只撤销未发布的本episode capability。两条slave-open route都先完成fd
  reservation、flags/access/admission与opened-description等fallible prepare，pair participation是最后一个可能失败的
  transition，随后conditional relation effect与fd commit不得再失败。master final release先在pair owner内不可逆retire，
  再在pair guard外以每pair唯一、exact且幂等的cleanup capability依次关闭devpts binding、撤销exact relation并形成旧session
  leader effect snapshot、提交`SIGHUP`/`SIGCONT`、唤醒各wait owner；不得跨owner rollback或恢复live state。
- **Protected API / ABI / contract：** `PTY-DEVPTS-CUTOVER`前，existing serial ABI与physical UART/console owner、
  `Terminal` truth、current `TTY-*`/`OPENED-DESC-*`/VFS/iomux/epoll contracts和generic serial `O_NOCTTY` baseline均保持
  不变。PTY UAPI codec只进入既有唯一ABI owner，kernel owner-local surface按真实consumer给最窄visibility；mount route
  必须保留generic `CAP_SYS_ADMIN` admission、任意existing-directory target与empty-data profile。不得注册半套namespace、
  建立dynamic final-release observer、per-mount backend/binding、devpts-local dentry freshness、第二份pair/readiness/refcount
  truth、success no-op ioctl或test-only production branch。
- **Acceptance / validation claim：** source/owner/lock/bypass review证明allocation/open/retirement的线性化、fallible prepare
  与guards-out effect；owner-local deterministic KUnit只证明singleton mount callback、data rejection、persistent
  last-view lifetime、capacity、rollback、safe reuse、stale episode、binding/metadata/admission与flag/ABI codec等纯语义，
  不把单任务测试外推为concurrent open-vs-retire、final-close、fd publication、signal
  ordering或generic pathname freshness证明。canonical RV64 TTY wrapper只证明existing serial与owner-local KUnit回归；LA64
  只取得repository kernel build claim。PTY test app、LTP、tmux、sshd与两架构PTY runtime均保持Not Run，留给Stage 4按父
  RFC acceptance取得实际证据。
- **Stop conditions：** 需要改变target/non-goals、owner/handoff/failure/cleanup、public ABI、visibility/shared contract、
  Contract Impact、acceptance或validation strength；结构拆分必须移动owner surface、扩大public API或建立新抽象层；VFS必须
  识别PTY private representation，pair/FileOps必须取得`Task`、file table或private VFS state；mount必须按target/path选择
  instance、建立per-mount binding或用mount count决定pair cleanup；safe reuse只能靠monotonic exhaustion或devpts-local
  dentry generation；implicit acquire必须让eligible/open failure混为一体；static final-release
  composition无法同时保留fanotify与PTY effect；cleanup必须恢复live state、持跨owner锁或启用excluded job-control residual；
  或必须提前公开半套namespace才能验证Stage 3。

预计实现自然涉及`device::tty`内的PTY pair/file/relation seam、new `fs::devpts`、VFS open activation的窄handoff、
`IoctlCtx`的窄fd installer capability、`task::files`既有reservation/static final-release consumer、`anemone-abi::tty`与kernel
Kconfig；devfs static publication和persistent init consumer只作为Stage 4 handoff做read-only seam核验，不属于Stage 3修改面。
这些是非穷举提示，不是逐文件write set；同owner新文件、必要import/re-export、inline KUnit与行为保持型拆分可自然闭合，
但不得借此清理相邻TTY/VFS代码或提前修改Stage 4 mount consumer。

### Resolved module boundary

1. **拆分`tty/pty.rs`，不拆owner。** 当前文件同时包含pair phase/participation、prepared/live lifecycle、master/slave
   description、FileOps、poll/read/write/ioctl与final-release composition；Stage 3继续加入lock/ioctl/cleanup会把state owner与
   opened-file adapter固化在同一文件。Checkpoint 1按pair lifecycle、opened-description/FileOps和composition root等稳定
   职责目录化，保持`device::tty` owner、现有consumer与visibility不变。具体文件名与private type placement由实现选择，
   不预建devpts callback、Stage 4 adapter或standalone test facade。
2. **拆分`tty/file.rs`的稳定角色。** generic TTY FileOps/operation、relation ioctl与termios/winsize ABI conversion已经是三类
   可独立review的职责；Checkpoint 1允许按这些角色目录化，使PTY-specific ioctl接入明确的owner-local dispatch seam。
   conversion仍服从`anemone-abi::tty`这个UAPI常量/布局owner，relation state仍只在relation registry；拆文件不复制codec或
   relation truth。
3. **不按行数扩散拆分。** `terminal.rs`仍围绕单一Terminal semantic owner，`anemone-abi/src/tty.rs`仍是单一TTY UAPI
   owner，serial attachment已经由endpoint/port边界表达；除非实现证据出现新的职责冲突，本Stage不为整齐继续拆这些文件，
   也不新建默认`tests.rs`/`kunit.rs`。owner-local KUnit继续inline放在被测语义末尾，跨子模块composition测试只放最低共同
   owner的`mod.rs`。

### Resolved integration route 与不变量

1. **Devpts instance、capacity、binding与episode identity。** new `fs::devpts`提供一个prebuilt persistent system instance、
   singleton superblock/root及current reserved/live episode capacity；capacity由kernel Kconfig的重要配置项拥有，具体默认值在实现时根据resource/acceptance证据选择，kernel必须
   验证非零且不超出accepted device-minor表示范围，外部构建工具不得clamp或重新解释语义。allocator只对current reservation
   计数；失败和retirement释放slot，capacity内反复allocate-close不能因历史churn耗尽。numeric index可以复用，但binding、
   inode/open capability和cleanup token都捕获immutable pair episode identity；旧capability只命中旧pair并fail closed，
   current binding解析new pair。devpts不读取、复制或推断VFS dentry freshness，generic cached-positive与late
   materialization继续由VFS register拥有。
2. **Allocation transaction先prepare后publish。** `/dev/ptmx` route先保留fd与capacity/index，捕获allocator operation-local
   `fsuid:fsgid`，prepare locked pair、master opened description、`0600`/character/`136:N` metadata与unpublished binding。
   所有显式返回`ENOMEM`、`ENOSPC`、fd/resource或enrollment错误的步骤都发生在pair/binding/fd visibility前；relation
   participant enrollment也必须在visibility前完成并由abort cleanup exact撤销。此后pair commit是第一个不可失败步骤，
   binding publication与master fd commit构成静态success tail；任一步prepare失败都不留下live pair、slave node、relation、
   participant或waiter。umask不改变accepted metadata profile，quota保持`ENOSPC`；显式fallible backing prepare的
   allocator failure保持`ENOMEM`，少量自然heap allocation可按父RFC R3工程约束在极端OOM时panic。
3. **Pathname open通过one-shot backend activation。** VFS继续完成pathname search、current credential inode-mode DAC、
   access/status decode与generic opened-description prepare；devpts inode只提供绑定exact episode的backend open capability。
   VFS open activation向`finish_open`携带一个窄、one-shot、backend-produced的publication capability，使FileDesc与static
   final-release effect全部prepare后才能提交pair participation；它不是dynamic hook registry，也不允许VFS识别PTY private
   type。`O_NOCTTY`作为typed operation-local slave-open effect只沿此route交给relation success tail，不进入File status
   truth，不改变generic serial baseline。
4. **`TIOCGPTPEER`通过窄fd installer。** `IoctlCtx`只增加reserve/commit本次新fd所需的capability，不暴露完整`Task`或
   file-table；PTY master FileOps先验证live master与accepted flags，peer route以master fd为capability，绕过pathname
   traversal与ordinary inode DAC，但与pathname route共享pair-owned lock/liveness/retirement和description participation。
   `O_CLOEXEC`、`O_NONBLOCK`、access mode与operation-local `O_NOCTTY`从validated flags形成；fd reservation、FileDesc、
   description/static hook与caller effect snapshot先prepare，pair participation最后fallibly commit，conditional implicit
   acquire与fd publication组成不失败tail。unsupported flag、bad master、locked/retired pair或fd exhaustion都不得留下
   participant/relation。
5. **PTY ioctl与implicit acquisition保持owner-local。** `TIOCGPTN`读取immutable allocation index，`TIOCSPTLCK`只修改
   pair-owned admission，`TIOCGPTPEER`使用上项route；asm-generic command/layout/flag codec只在`anemone-abi::tty::linux`
   定义一次，kernel不复制raw number。relation owner提供successful slave-open专用的conditional effect：只有未设置
   `O_NOCTTY`、readable、current caller是session leader、caller session无controlling terminal且endpoint未绑定其它session
   时原子建立relation并设置caller current process group为foreground；不eligible、竞争失败或已存在关系都只形成no-op，
   不让已经成功的slave open失败，也不改变后续explicit `TIOCSCTTY`。
6. **Master retirement使用固定cleanup capability。** allocation时为每pair静态组合唯一cleanup capability，并继续在
   current single `FileDescOps::final_release`内组合既有fanotify base effect；不增加observer registry或第二个task-owned
   hook。master final release先让pair不可逆retired、禁止admission并发布hangup predicate，随后在pair guard外exact retire
   devpts binding；relation owner按旧generation与stable session-leader identity先撤销`/dev/tty`/foreground discoverability，
   再返回不可变effect snapshot；Signal与ThreadGroup owner依次接收`SIGHUP`、`SIGCONT`，最后各read/write/poll wait route
   被提示重验durable predicate。cleanup重复、迟到或与index reuse竞争时只能命中旧episode，不能撤销new binding/relation或
   恢复old pair。
7. **Mount operation只投影singleton。** Stage 3的unregistered production mount callback只接受empty data，并总是返回同一
   prebuilt persistent superblock/root；它不读取target pathname、current credential、mount namespace或mount count，也不
   创建per-mount allocator/binding。distinct `Mount` identity及view attach/detach由VFS拥有；single-view unmount不影响其它
   view，last-view unmount不kill instance，remount取得current namespace。`newinstance`与其它nonempty option稳定返回
   `EINVAL`；global `/dev/ptmx`不按path选择instance，mount root不增加`ptmx`node。
8. **Stage 4 mount handoff只自动配置persistent system `/dev`。** live consumer audit区分了长期system-devfs与临时
   pre-chroot devfs：`final-entry`、`busybox-init`、`user-test` chroot后的测试环境与`tty-test`属于persistent consumer；
   `board-init` pre-chroot磁盘发现和`user-test` pre-chroot测试盘挂载属于temporary consumer。Stage 3只交付尚未注册的
   mount-ready instance与static publication capability；Stage 4才注册filesystem、让devfs预发布`ptmx`/empty `pts`并由
   persistent init显式mount `/dev/pts`。其它`CAP_SYS_ADMIN`调用者可在任意existing directory建立additional view；kernel
   不把所有devfs mount点机械改成devpts consumer，也不自动创建ordinary-filesystem mountpoint。

### Checkpoint 1 — TTY / PTY owner-local structural split

**Purpose：** 在引入devpts、VFS activation与新ioctl前，把现有TTY/PTY职责按上文稳定角色目录化，为后续review建立清楚的
state owner与adapter边界。该checkpoint独立安全并保持全部production行为；只授权本checkpoint时，关闭后立即停止。

**Deliverable：**

- `pty`模块清楚区分pair lifecycle/participation与master/slave opened-description/FileOps，`file`模块清楚区分generic
  operation、relation ioctl与termios/winsize conversion；existing production caller全部切到新布局；
- private visibility、public/re-export surface、type/trait semantics、lock/lifecycle/final-release顺序、KUnit placement与serial/
  PTY behavior保持不变；不增加unused facade、compat wrapper、old/new双路径或只为Checkpoint 2预留的抽象层；
- source review确认拆分后`Terminal`、pair、relation与opened-description仍各有一份truth，模块依赖方向没有让FileOps读取
  pair guard、relation private state或task/VFS owner。

**Validation：**

1. `git diff --check`、`just fmt kernel --check`与`mdbook build docs`；
2. repository RV64 kernel/KUnit与canonical TTY wrapper通过existing serial `50/50`、vi/ash、host byte oracle及正常关机；
3. canonical LA64 repository kernel build通过；source/module/bypass audit证明diff只改变同owner布局，external behavior、
   visibility/shared contract与Stage 2 capability均未改变。

**Cutover / Exit：** None。全部existing consumer迁移到单一路径、validation与Architecture Friction Scan关闭后，Checkpoint 1
可记为Closed并立即停止；Stage 3尚未关闭，Checkpoint 2仍需单独授权。若拆分暴露必须改变owner/public API/contract
或保留过渡adapter才能工作，停止并上报，不把它伪装成结构维护。

**Execution Result（2026-08-11）：** Closed。`tty/pty.rs`按composition root、pair lifecycle/participation与opened-description/
FileOps拆为目录模块；`tty/file.rs`按generic operation、relation ioctl与termios/winsize ABI职责拆为目录模块。existing
module entry、consumer、有效visibility、type/trait semantics、pair/Terminal/relation/opened-description truth与
final-release顺序保持；master FileOps只消费description capability，不再穿透pair guard或private fields。原
`PtySlaveDescription`的`Opaque` marker保留，termios-only KUnit随被测语义移入`file/termios.rs`，PTY composition KUnit仍在
最低共同owner。

source/module/bypass audit与独立review最终为0 Apollyon / 0 Keter / 0 Euclid / 0 Safe；Architecture Friction Scan未发现需
保留的具体摩擦。final candidate通过`git diff --check`、`just fmt kernel --check`与`mdbook build docs`；canonical RV64
wrapper完成609/609 KUnit、existing serial `TTYTEST:SUMMARY:PASS:50`、BusyBox vi/ash、host byte oracle与orderly shutdown；
canonical LA64 repository build通过，final symbol table为6451 entries。没有devpts/ptmx/UAPI、Checkpoint 2 dormant surface、
contract cutover或register变化；LA64 runtime、PTY app、LTP、tmux与sshd保持Not Run。执行在Checkpoint 1停止，Checkpoint 2
仍需维护者单独授权。

### Checkpoint 2 — Hidden devpts / open / cleanup integration

**Purpose：** 在Checkpoint 1的新布局上实现全部Stage 3 production route与跨owner handoff，同时保持devpts未注册、
`/dev/ptmx`与`/dev/pts`未发布，为Stage 4留下单一、可review的公开激活点。

**Deliverable：**

- single persistent devpts instance/superblock、empty-data mount callback、last-view lifetime、capacity/index/live binding、
  safe reuse、accepted initial metadata与mount-ready VFS projection关闭；
- hidden `/dev/ptmx` allocation composition、PTY ioctl codec、pathname one-shot activation、`TIOCGPTPEER` fd installer、shared
  pair admission/participation与implicit relation effect全部走production owner path，失败rollback无残留；
- master final release静态组合fanotify与pair cleanup，retirement后exact完成devpts/relation/Signal/job-control/waiter handoff，
  stale episode不能命中新binding或relation；
- devpts filesystem registration、devfs `ptmx`/empty `pts` publication和persistent mount consumer均不激活；Stage 4只需连接这些static
  activation point并运行public acceptance，不保留另一路径或重新实现allocation/open policy。

### Validation 与 closure

1. `git diff --check`、`just fmt kernel --check`与`mdbook build docs`；
2. source/owner/lock/bypass audit逐条追踪singleton mount callback、superblock/last-view lifetime、mount-neutral pair cleanup、
   allocation rollback与success tail、pathname/peer route-specific precondition与shared admission、fd publication、
   implicit-acquire no-fail tail、static final-release composition、relation effect snapshot、retire/reuse/stale capability及
   guards-out cleanup；明确generic VFS cached-positive/late-materialization/full multi-view linearizability仍为Not Proven；
3. inline owner-local deterministic KUnit覆盖same-superblock mount、empty/nonempty data、persistent last-view lifetime、capacity
   exhaustion/release、failed allocation rollback、index reuse、old episode
   fail-close、exact binding retirement、metadata/admission、两route flag codec与纯snapshot/transition；测试不进入live
   scheduling、timer、sleep或wait，不新建standalone test facade，也不扩大production visibility；
4. concurrent allocation/open-vs-retire、final close、fd-publication race、relation/signal ordering、lost-wake与lock order只由
   source/linearization review证明；不得用单任务KUnit、固定yield/sleep或repository KUnit总数声称runtime interleaving proof；
5. canonical RV64 TTY wrapper完成repository build、符合第3项边界的KUnit、existing serial `50/50`、BusyBox vi/ash、host
   byte oracle与正常关机；结果不外推为未公开PTY runtime proof；
6. canonical LA64 repository kernel build通过；LA64 runtime、PTY Rust test app、LTP、tmux、sshd与所有public PTY pathname/
   ioctl runtime保持Not Run，且不得从source、KUnit、RV64 serial或双架构build外推；
7. stage-wide review与Architecture Friction Scan检查第二份pair/binding/readiness/refcount truth、owner穿透、VFS或caller/arch/
   test特判、dynamic hook、无退出条件bridge、隐含cleanup顺序与用弱化oracle换取隐藏集成；Apollyon/Keter必须在closure前消除
   或触发停止，未在边界内消除的Euclid按workflow写回。

**Cutover / Exit：** None。只有Checkpoint 1先按独立授权关闭，且随后devpts、allocation/admission、两条slave-open route、
PTY ioctl、implicit acquisition、static final-release与cross-owner cleanup全部满足上述production/source/KUnit/build证据，
Checkpoint 2与Stage 3才可记为Closed并立即停止。关闭不注册devpts、不发布PTY namespace、不修改current contract/register，
也不自动授权Stage 4；public activation与mandatory PTY test app/LTP/tmux evidence仍需维护者单独授权Stage 4。若隐藏集成无法
在无半套publication下证明，或Stage 4必须重写而非连接static activation point，必须在Stage 3完成声明前回到RFC review。

**Execution Result（2026-08-11）：** Closed。新增尚未注册的`fs::devpts` production owner：prebuilt persistent
superblock/root、empty-data mount route、configured live/reserved capacity、reusable index、exact episode binding、accepted
metadata与retired inode reclaim保持一份backend truth；VFS继续拥有mount view、inode/dentry与pathname cache。hidden
`/dev/ptmx` allocation先保留fd/index并prepare pair、relation enrollment、inode/binding与static final-release composition，
随后以pair commit、binding publication和fd commit形成不失败success tail。显式capacity、fd、VFS index、enrollment与
fallible backing prepare仍返回诚实errno并在publication前rollback；少量自然heap allocation按父RFC R3允许在极端OOM
时panic，不为形式上的recoverability扭曲owner或commit形状。

pathname slave open通过backend-produced one-shot activation进入shared pair admission/participation，`TIOCGPTPEER`通过
`IoctlCtx`窄fd reservation/commit capability复用同一admission与implicit relation effect；`TIOCGPTN`、`TIOCSPTLCK`与
peer-open flags只在既有TTY UAPI owner解码。raw kernel `PathRef::open()`不能安全提交activation，因此稳定返回
`NotSupported`并撤销prepared activation；fanotify把该结果投影为`FAN_NOFD`，没有产生PTY participation或改变其metadata/fd
transaction。master final release先在pair owner内retire，再在guards-out cleanup中exact撤销binding、回收retired inode、
retire relation并提交`SIGHUP`/`SIGCONT`及waiter recheck；迟到/repeated cleanup不能命中新episode。

review依次关闭natural allocation形状、fanotify activation transaction、last-view lifetime、retired inode reclaim、relation
guard外effect、locked admission与冗余phase/test facade问题。最终独立review为0 Apollyon / 0 Keter / 0 Euclid / 0 Safe；
Architecture Friction Scan未发现需要保留的具体摩擦。final candidate的canonical RV64 wrapper通过623/623 KUnit、existing
serial `TTYTEST:SUMMARY:PASS:50`、BusyBox vi/ash、host byte oracle与orderly shutdown；该runtime结果不外推未公开PTY。
canonical LA64 repository build通过，final symbol table为6588 entries。`git diff --check`、`just fmt kernel --check`与
`mdbook build docs`通过。

LA64 runtime、PTY Rust test app、LTP、tmux、sshd与所有public PTY pathname/ioctl runtime保持Not Run。generic VFS
cached-positive freshness、late materialization、完整multi-view pathname linearizability，以及concurrent allocation/
open-vs-retire、final close、fd publication、relation/signal ordering、lost-wake与lock order的runtime interleaving均为
Not Proven；后者只取得source/lock/linearization review。devpts filesystem registration、devfs `ptmx`/empty `pts`
publication、persistent init consumer、current contract/register与`PTY-DEVPTS-CUTOVER`保持不变。执行在Stage 3 closure
立即停止；Stage 4保持Future且未获授权。

## Stage 4 — Public activation、acceptance 与 `PTY-DEVPTS-CUTOVER`

**Purpose：** 原子注册user-mountable devpts filesystem，由devfs公开static `/dev/ptmx`与empty `/dev/pts` mountpoint，
persistent init显式mount canonical system view，并公开完整R3 operation surface；运行mandatory
source/owner、PTY Rust test app、LTP和architecture evidence，尝试并归因tmux，随后一次性完成父RFC列出的
PTY/DEVPTS/TTY contract Introduce/Refine并关闭RFC；sshd仅在条件具备时作为诊断consumer。

**Prerequisites：** Stage 3关闭；全部production path、rollback/cleanup和acceptance consumer已在同一可review状态；
维护者另行授权Stage 4与`PTY-DEVPTS-CUTOVER`。

**Protected Boundary：** core claim缺失时不得部分cut over或用tmux/sshd环境结果改写target；RV64/LA64 build/runtime分别
记录，只有实际运行的架构取得runtime-proven claim；generic VFS pathname freshness继续留在register，current limitation
只按真实LTP/ABI证据缩减。Stage 4必须证明canonical与一个ordinary-directory fresh view共享instance/binding；system-mount
consumer修改只落在persistent init，不触达temporary pre-chroot consumer。不要求full cached/reuse multi-view
linearizability。到达Stage 4前不冻结artifact identity、精确命令或未要求的额外runtime范围。

## 证据与反馈路由

- target、owner、ABI、Contract Impact与acceptance变化只写父RFC并经review；implementation route correction写本页。
- 每个Stage的实际代码、测试、命令与结果优先由Git/PR保存；只有长期多Stage执行历史、probe或renegotiation确有需要时
  才创建transaction。
- `PTY-DEVPTS-CUTOVER`前不更新current contract；cutover时只更新父RFC列出的真实Introduce/Refine ID。
- 新发现的当前缺陷或接受限制进入register；VFS dynamic positive-dentry issue不因PTY source/test通过而关闭。
- 每个Stage收口前执行Architecture Friction Scan；只有具体Euclid/Keter/Apollyon证据才写回本页或RFC closure，Safe不
  留占位结论。
