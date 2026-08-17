# RFC-20260817-unix-scm-rights

**状态：** Closed
**修订：** R0
**负责人：** doruche, Codex
**最后更新：** 2026-08-17
**领域：** Socket / Unix IPC / task opened description / syscall ABI
**影响契约：** Introduce `OPENED-DESC-TRANSFER-001`、`UNIX-SOCKET-RIGHTS-001`；Refine `OPENED-DESC-001`、`OPENED-DESC-003`、`OPENED-DESC-RETIRE-001`、`SOCKET-ABI-001`、`UNIX-SOCKET-STREAM-001`、`UNIX-SOCKET-LIFECYCLE-001`；全部 Effective
**执行记录：** Git commit `scm-rights: implement Unix stream fd passing`；`UNIX-SCM-RIGHTS-CUTOVER` Effective

## 摘要

本 RFC 为 native RV64/LA64 的 `AF_UNIX + SOCK_STREAM` 引入 `sendmsg(2)` / `recvmsg(2)` 与
`SOL_SOCKET/SCM_RIGHTS` 文件描述符传递。Socket ABI adapter统一拥有 Linux `msghdr/cmsghdr`、用户指针、对齐、
flags、errno与copy ordering，并通过现有immutable static `SocketOps` descriptor分发一项窄的rights capability；只有
Unix stream direction实现字节与rights的共同transaction。其它Socket family不获得ancillary storage、runtime registry
或generic control-message bag。

`task::files`提供opaque、move-only的transferable opened-description reference，唯一负责从发送者fd table捕获exact
opened description、保持跨sender close的semantic lifetime、为接收者预留和安装fd slot，以及abort/final release。Unix
direction只保存经过准入的opaque rights bundle，并把它与一个确定的stream byte position一次提交；它不接收`Task`、
`ProcFile`、fd-table lock或Linux ABI对象。

R0明确拒绝传递任何Unix Socket fd，因此不引入AF_UNIX inflight graph、cycle detection或garbage collector。偏僻Linux
边角若需要第二份状态真相、跨owner回滚、通用框架或不自然的代码形状，本RFC优先给出ABI诚实的拒绝、fail-forward或
accepted limitation；但opened-description引用正确性、字节/rights绑定、并发、cleanup、内存安全和错误返回诚实性不能
以工程妥协削弱。

R0 target同时要求在cutover前闭合现有Unix direction retirement暴露的lifecycle缺口：`SHUT_RD`只关闭未来ingress，endpoint仍published且
已经排队的payload继续可读；semantic final release先撤销endpoint association，随后由stream或seqpacket direction
detach只流向该retired endpoint、因而已经不可达的inbound payload。该修复覆盖stream与seqpacket的既有payload lifecycle，
但不向seqpacket发布rights transport，不建立新的consumer-alive状态，也不形成独立cutover。

## 背景

当前 [Socket front contract](../../contracts/socket/front-abi-wait.md) 规定raw message header、flags、user copy与errno
止于family-neutral adapter，concrete family只接收normalized request并返回typed outcome；static `SocketOps` descriptor
是semantic type与capability的唯一witness。live `sendmsg`在`msg_controllen != 0`时统一返回`EOPNOTSUPP`，Unix stream
ABI profile尚未发布message I/O。当前
[Unix stream contract](../../contracts/socket/unix-stream-lifecycle.md#unix-socket-stream-001--direction-owner提交stream与shutdown结果)
只拥有bounded byte deque、terminal、capacity和routes；send/receive均在user copy后经operation gate与current-association
recheck提交prefix。

当前 [opened-description contract](../../contracts/task/opened-description-lifecycle.md) 以published fd slot reference作为
semantic lifetime与final release的唯一真相。普通syscall-local `Arc<FileDesc>`不会延迟final release；这对close/dup/fork
是正确的，但不足以表达SCM_RIGHTS：发送者在成功send后可以关闭原fd，而队列中的rights必须继续保持同一opened
description及其file offset、status flags、flock和static final-release lifecycle，直到接收者安装或所有未交付引用被释放。
该能力必须进入`task::files`的唯一lifecycle owner，不能由Unix Socket用`Arc<File>`、`FileDesc` clone或另一份引用计数模拟。

当前Unix stream/seqpacket在`SHUT_RD`后保留已经排队的bytes/records，current contract与owner-local KUnit都要求buffered
payload先于terminal结果交付；但live final-release route只提交与`SHUT_RD`相同的reader terminal fact，没有在endpoint
association撤销后detach该endpoint已经无法消费的inbound queue。只要peer继续持有connection，最多一个direction budget的
普通payload会保持不可达；加入SCM_RIGHTS后，同一缺口还会无消费者地延长opened-description semantic lifetime。固定Linux
6.6.32的`unix_shutdown()`同样只更新shutdown facts，而`unix_release_sock()`才flush receive queue并释放passed fds。本R0
把这一既有lifecycle修复作为rights实现的前置slice纳入同一implementation unit。

固定 Linux 6.6.32 参考中，`scm_fp_copy()`按输入顺序取得file references，`unix_stream_sendmsg()`只把rights放入第一个
实际发送的非空buffer，普通stream receive在消费到携带rights的buffer时detach且一次停止于一个rights group，
`MSG_PEEK`则复制references；接收control不足时设置`MSG_CTRUNC`并释放未安装references。依据见
`xref:linux-6.6.32:net/core/scm.c#scm_fp_copy`、
`xref:linux-6.6.32:net/core/scm.c#scm_detach_fds`、
`xref:linux-6.6.32:net/unix/af_unix.c#unix_stream_sendmsg`与
`xref:linux-6.6.32:net/unix/af_unix.c#unix_stream_read_generic`。这些事实只作为ABI与可见行为参考，不规定Anemone
内部使用Linux的`sk_buff`、`scm_cookie`、inflight accounting或GC形状。

## 目标

- 只为connected `AF_UNIX + SOCK_STREAM + protocol 0`发布native `sendmsg/recvmsg`与`SCM_RIGHTS`；同时覆盖unnamed
  socketpair和pathname connect/accept形成的stream connection。
- 修复stream与seqpacket的endpoint retirement：`SHUT_RD`只关闭未来ingress并保留已经排队的inbound payload；semantic
  final release在撤销endpoint association后detach该端不可达的inbound payload，同时保留peer仍可读取的outbound payload。
- Socket ABI adapter解析并编码native LP64 control messages，支持一个或多个`SOL_SOCKET/SCM_RIGHTS` cmsg，并把其中
  fd按输入顺序合并为一个typed rights bundle。
- `task::files`提供exact capture、semantic transfer reference、dup-for-peek、receiver reservation/install、abort与terminal
  release；接收fd继续共享原opened description的position、status flags、access/compat和backend lifecycle。
- Unix stream direction把一个rights bundle与本次send实际提交的第一个byte position原子绑定；partial positive send只交付
  一次rights，zero progress不移交rights。
- `recvmsg`在返回的byte prefix首次覆盖rights position时最多交付一个rights group；普通read/recv消费到同一position时
  丢弃并释放rights，不能使隐藏引用永久滞留。
- 支持`MSG_CMSG_CLOEXEC`、`MSG_CTRUNC`和`MSG_PEEK`；重复peek每次可以安装一组新的fd，但不消费原bytes或queued rights。
- rights queue有独立、可配置且有界的per-message/per-direction容量；blocking rights send使用operation-specific predicate，
  nonblocking不足返回`EAGAIN`，不busy-poll、不把普通stream `WRITABLE`改成request-sized truth。
- 用一次连续implementation unit和唯一`UNIX-SCM-RIGHTS-CUTOVER`同时完成代码、current contract、register处置、源码审查、
  owner-local KUnit和双架构定向`socket-test`；不发布只有parser或transfer handle但用户能力不完整的中间ABI。

## 非目标

- 不允许通过`SCM_RIGHTS`传递任何AF_UNIX Socket fd，包括当前Unix stream与seqpacket fd；一个bundle中只要出现此类fd，
  整次send在payload commit前返回`EOPNOTSUPP`。
- 不实现Unix datagram、Unix seqpacket或其它Socket family上的rights transport；seqpacket只参与既有endpoint-retirement
  lifecycle修复，非Unix Socket fd仍可作为被传递的普通opened description。
- 不实现`SCM_CREDENTIALS`、`SCM_PIDFD`、`SO_PASSCRED`、security label、timestamp、error queue组合或其它ancillary
  producer/consumer。
- 不建立generic ancillary record、mutable option/control bag、runtime family registry、callback bus或future-family slot。
- 不实现AF_UNIX inflight graph、cycle detection、garbage collector、per-user inflight accounting或Linux internal
  `sk_buff/scm_cookie`同形结构。
- 不增加compat32 cmsg、`recvmmsg`、Unix OOB、abstract namespace、autobind、Unix datagram/seqpacket message target或新的
  fd namespace/resource-limit模型。
- 不承诺Linux对非`SOL_SOCKET` cmsg的忽略规则、非`i32`整数倍rights payload的截尾解释、mixed-invalid precedence、
  重叠用户buffer、control copy fault后的partial fd installation或其它偏僻副作用同形；本RFC为这些输入给出下面明确的
  Anemone语义。
- 不借机重构TCP/UDP/netlink message I/O、Unix namespace/admission、opened-description dynamic observer、flock、epoll或
  整个fd table。

## Owner 与协议边界

### 状态与能力所有权

| 状态 / 能力 | 唯一 Owner | 其它参与方持有什么 | 行为用途 |
| --- | --- | --- | --- |
| raw `msghdr/cmsghdr`、alignment、flags、errno与user copy | Socket ABI adapter | family只取得normalized stream/control request与typed outcome | Linux ABI解析、copy ordering与结果投影 |
| static rights capability | immutable `SocketOps` descriptor | Socket front读取capability presence | 只向Unix stream分发rights transaction |
| fd slot与exact opened-description capture | sender `task::files` / file-table episode | Socket ABI取得opaque transfer candidate/bundle | 防止fd reuse并建立semantic transfer reference |
| opened-description semantic lifetime与final release | `ProcFile` lifecycle owner | published slot或move-only transfer reference | sender close后保活、receiver install、abort与terminal cleanup |
| endpoint association与consumer reachability | `UnixEndpointCore` | connection只在association撤销后取得side-specific retirement handoff | 区分`SHUT_RD` ingress terminal与final-release不可达cleanup |
| stream bytes、byte capacity、rights position、rights capacity、terminal与routes | 对应Unix stream direction | front只收发opaque bundle与typed result | byte/rights共同commit、consume/peek、wait与drain |
| seqpacket records、capacity、terminal与routes | 对应seqpacket record direction | endpoint只提交side-specific retirement handoff | 保持现有record ABI并detach retired endpoint的不可达inbound payload |
| receiver fd reservation与publication | receiver `task::files` / file-table episode | ABI adapter持move-only reservation/install plan | fd number分配、CLOEXEC与不可失败publication |

Socket ABI不得从Unix private state判断family，也不得让profile与descriptor分别维护rights truth。static descriptor的
capability absence是永久不支持的唯一见证；Unix endpoint role-dependent的unconnected/retired/peer-close结果仍由family
owner返回typed rejection。Unix owner不得检查fd number、receiver rlimit、control capacity或用户指针。

### Opened-description transfer lifecycle

一次capture在sender file-table episode guard内按fd数组顺序解析exact live slots，并为每项建立由`task::files`拥有的
semantic transfer reference。它不是non-owning liveness lease，也不是普通`Arc` borrow；只要published slot或transfer
reference任一仍存在，同一opened description就不得进入terminal retirement。lifecycle owner必须以一份状态统一核算两类
reference，不能让Unix queue保存第二份alive bit、raw `ProcFile`或独立final-release counter。

capture完成后先释放fd-table guard，再由Socket front通过transfer candidate提供的窄immutable file-kind admission判断是否
命中AF_UNIX Socket；该判断不能暴露`ProcFile`、lifecycle word、fd-table container或允许Unix owner调用任意VFS operation。
坏fd返回`EBADF`，任一AF_UNIX Socket返回`EOPNOTSUPP`并发出限频notice；任一失败都释放本次已经capture的全部references，
不复制payload、不提交bytes或rights。

receiver install先在其file-table episode中取得所需reserved slots，形成共享原opened description且带receiver-local
`FdFlags`的unpublished descriptors。用户可见fd number与全部message output copy成功后，reservation与transfer reference
以不可失败、exactly-once的owner API转换为published slot；该转换不得经历“transfer已释放而slot尚未取得reference”的
terminal gap，也不得在fd-table guard内调用VFS、Socket、flock或backend final-release。reservation/plan的Drop只回滚尚未
publication的slot并释放仍由plan拥有的transfer reference。

`MSG_PEEK`由`task::files`复制semantic transfer references形成新的operation-local bundle；queued original继续唯一表示
尚未消费的rights。重复peek不会复制Unix direction state，只会在每次成功recv transaction中形成新的receiver fd aliases。

### Send handoff 与线性化

Socket ABI先完整解析control并capture全部source descriptions，再进入现有stream staged-copy/retry。一个非空rights bundle
需要同时满足至少一个byte capacity和完整rights-count capacity；rights不能partial enqueue。direction capacity不足时，
nonblocking send返回`EAGAIN`且bundle仍由本次syscall持有，blocking send释放所有operation guard后等待
operation-specific predicate，再重新执行current-association、terminal与capacity检查。

一次positive send commit在同一个direction owner window内完成：先确定commit前tail byte position，再共同追加成功复制的
byte prefix与指向该首byte的rights marker，同时更新rights charge。任何shutdown、peer read-close、retirement或stale
association在commit recheck前胜出时，不得留下bytes-only或rights-only publication。partial positive send只在首个已提交
prefix携带整个bundle并立即返回short success；后续用户重试属于新的message，不会重复本bundle。

zero-length payload仍先执行message/control/fd validation；若全部有效，`sendmsg`返回0并释放captured bundle，不创建无byte
marker、不占用direction capacity。这与固定Linux stream参考一致，也避免为不可定位的control-only event增加第二套record
语义。

### Receive handoff、peek 与fail-forward

direction read gate先选择可复制的stable byte prefix。若prefix尚未覆盖下一个rights marker，结果不携带rights；若覆盖，
本次最多包含该marker并在下一个rights marker前停止，因而一次receive最多移交一个rights group。zero-length receive不观察、
复制、消费或丢弃当前marker。

payload copy fault继续沿用stream当前规则：未完成direction commit，不消费bytes或rights。payload copy成功后：

- 非peek在同一个direction commit中消费已复制prefix、摘除其中唯一rights marker并释放对应capacity；marker与bundle所有权
  一起移交给Socket ABI，之后绝不requeue。
- peek不消费bytes、marker或capacity；direction复制semantic references并把operation-local bundle移交给ABI。复制失败返回
  对应资源错误，queued original保持不变。
- 普通`read/readv/recv/recvfrom`没有control consumer；非peek commit摘除的bundle由front在任何Socket/connection guard外
  直接释放。`recvmsg`没有可写control空间时同样消费并释放bundle，但设置`MSG_CTRUNC`。

direction commit后的name/control/header copy属于Socket ABI fail-forward。ABI先决定control可容纳的fd前缀，再按receiver
当前fd ceiling预留尽可能多的slot；没有slot或空间不是`EMFILE`失败，而是成功返回payload、设置`MSG_CTRUNC`并释放未安装
references。对于计划安装的前缀，ABI在publication前完成peer name、native cmsg、fd number、`msg_flags`与
`msg_controllen`的全部可失败copyout；任一copy fault返回`EFAULT`、回滚全部本次reservations且不发布新fd。非peek已经
消费的bytes/rights不回队，未安装bundle被释放；peek的queued original保持不变。全部copy成功后才执行不可失败fd
publication。

这一“direction consume/detach后ABI fail-forward、fd publication all-or-none”是R0有意语义。它保持现有Socket
`payload transaction -> name/control/header copy`形状，避免跨Unix read gate持有receiver fd-table能力，也避免Linux
control fault可能产生的partial installation。若真实consumer要求另一种副作用，必须以证据重新评估target，不能在adapter
中补偿requeue或让用户fd publication先于direction ownership handoff。

### Capacity、wait 与cleanup

R0使用两个production Kconfig：`unix_scm_rights_max_fds_per_message`默认253，
`unix_scm_rights_direction_capacity_fds`默认1024；acceptance配置固定使用这两个默认值。两者必须非零，direction capacity
必须不小于per-message maximum，并且所有count/charge arithmetic必须checked。超过per-message maximum返回`EINVAL`，
不会capture部分fd。具体配置schema与生成常量名属于implementation preference，但这些重要上界不得变成hidden literal。

public stream `WRITABLE`继续只表示普通byte send至少可以取得progress或已terminal，不承诺任意rights bundle可容纳。
rights-bearing blocking send使用包含requested rights count的operation-specific wait；rights consume/drop/drain释放charge后
必须对相应route发出recheck hint。hint不携带capacity truth，wake后仍由direction live predicate裁决。

`SHUT_RD`只关闭对应direction的未来ingress，使peer后续send得到`EPIPE`；它不撤销endpoint association，已经排队的
bytes/records与rights仍由该endpoint合法消费，读尽后才观察terminal。semantic final release则先撤销endpoint association，
再由connection/direction在同一个retirement commit中detach只流向该endpoint的全部inbound payload并归还capacity；closing
endpoint已经提交到outbound direction的buffered payload必须保留，供仍存活peer读取。stream与seqpacket共享这一reachability/
retirement规则，但只有stream payload可以携带rights。listener child retirement沿用同一endpoint route，connection最终销毁
释放任何剩余payload。

所有可能触发opened-description terminal retirement、VFS/flock cleanup或Socket final release的bundle/drop都在fd-table、
endpoint与connection spinlock之外执行；notify同样使用guard外snapshot。send abort（包括terminal recheck失败）、短control、
fd exhaustion、copy fault、ordinary read、peer/final close、listener child cleanup与connection retirement的每个transfer
reference都必须有唯一cleanup owner。

## R0 Correctness Invariants

- **单一lifecycle truth：** published slot与transfer reference由`task::files`同一opened-description lifecycle owner核算；
  `Arc` storage、Unix queue长度、fd number或receiver reservation都不能替代semantic liveness。
- **exact capture：** source fd在同一file-table episode guard内解析并取得reference；fd close/reuse不能让一次send capture后来
  复用该数值的新description，也不能让成功capture在sender close后失效。
- **byte/rights共同commit：** 非空bundle只能与一个positive byte prefix在同一direction update中发布；不存在queued
  rights without byte、bytes-only stale commit或两个独立rollback点。
- **receive ownership单向移交：** 非peek consume把bytes与命中的bundle一起从direction移交给ABI；成功移交后不requeue。
  peek只复制references，不改变queued truth。
- **reachability与terminal分离：** `SHUT_RD`只关闭未来ingress并保留queued payload；只有endpoint association撤销后的
  retirement handoff才detach本端不可达inbound payload。endpoint association继续是consumer reachability唯一truth，
  direction不得增加并列alive状态。
- **publication无失败尾巴：** receiver fd slot只在全部相关copyout成功后由reservation与transfer reference不可失败转换；
  syscall不得返回错误同时留下本次新published fd。
- **cleanup guard外：** 任一transfer reference释放都不能在fd-table、Unix endpoint/connection或Socket owner guard内触发外部
  cleanup；每个abort、truncate、discard、endpoint/listener/connection retirement路径exactly once释放。
- **bounded与无busy retry：** queued rights charge永不超过Kconfig上界；request-specific capacity不足使用typed
  not-ready/wait，不以unbounded queue、panic、普通WRITABLE busy loop或silent rights drop吸收。
- **ABI诚实：** unsupported transport/control/AF_UNIX source fd在payload commit前显式失败；target内成功结果不能隐藏
  references泄漏、错误CLOEXEC、未报告truncation或不完整publication。

这些invariants内联于本页即可完成review与proof mapping；当前不需要单独`invariants.md`。若实现证据表明正文无法保持可扫读，
可以只做文档布局拆分，但不得借拆分改变target、owner或acceptance。

## ABI 与可见语义

### 支持面

- transport只包括connected `AF_UNIX + SOCK_STREAM + protocol 0`的socketpair与pathname connection。
- message calls包括`sendmsg`与`recvmsg`；现有`sendmmsg` wrapper在一个稳定opened description上逐项复用独立的`sendmsg`
  transaction，不增加batch ancillary state或跨message原子性。positive partial stream element写入实际`msg_len`、计为一个
  completed element并停止后续项，rights bundle只随该element提交一次；send commit后的`msg_len` copyout fault沿用既有
  fail-forward：首项返回`EFAULT`、已有completed element时返回此前计数，当前已提交bytes/rights不requeue或自动重放。
  `recvmmsg`不在R0。
- 无control的Unix stream `sendmsg/recvmsg`与现有write/read byte semantics一致。connected stream的nonempty send
  destination返回`EISCONN`；recvmsg请求name时由ABI adapter投影connection已有immutable peer-name snapshot，unnamed peer
  返回empty name，不让raw sockaddr进入Unix owner。
- send flags继续支持`MSG_DONTWAIT | MSG_NOSIGNAL`；recvmsg增加
  `MSG_DONTWAIT | MSG_PEEK | MSG_CMSG_CLOEXEC`。其它未知或未支持flags返回`EOPNOTSUPP`。

### Send control parsing

- `msg_controllen == 0`表示没有control；nonzero length要求可读`msg_control`并按native `CMsgHdr` size/alignment遍历。
- 每个有效cmsg必须是`SOL_SOCKET/SCM_RIGHTS`。well-formed但其它level/type返回`EOPNOTSUPP`；header短、length越界、
  alignment/offset overflow或rights data不是完整native `i32`数组返回`EINVAL`；用户读取失败返回`EFAULT`。
- 一个或多个rights cmsg按出现顺序合并；zero-fd rights cmsg是no-op。总fd数超过配置maximum返回`EINVAL`。
- 任一数值fd不存在返回`EBADF`；任一capture结果是AF_UNIX Socket返回`EOPNOTSUPP`并发出一次限频诊断。mixed list
  整体失败且不提交payload；本R0不把所有mixed-invalid error precedence提升为不变量，至少保证按解析/capture顺序稳定、
  不因并发fd reuse观察另一description。
- sender fd-local `FD_CLOEXEC`不随opened description传递；共享position、status flags、access mode和backend state。

### Receive control projection

- rights存在且control能完整表达至少一个fd时，成功结果最多输出一个`SOL_SOCKET/SCM_RIGHTS` cmsg；`cmsg_len`按实际安装
  fd数使用native `CMSG_LEN`，`msg_controllen`报告实际占用的native control span，未写padding保持零初始化或不被读取。
  若最终安装零个fd，则不输出空rights cmsg，`msg_controllen`为0并设置`MSG_CTRUNC`。
- control缺失、短于header、只能容纳fd前缀或receiver fd slots不足时，安装可完整表达且成功预留的最大前缀，其余释放；
  `msg_flags`增加`MSG_CTRUNC`。零个slot仍返回payload success而不是`EMFILE`。
- `MSG_CMSG_CLOEXEC`只决定本次接收fd的`FD_CLOEXEC`；没有该flag时新fd默认不带CLOEXEC。
- `MSG_PEEK`每次成功调用都可产生新的fd numbers并共享同一opened description；bytes、marker与queued reference不消费。
- nonpeek的payload copy fault发生在direction commit前，bytes/rights保持；peer-name、control或output-header copy fault发生在
  direction commit后，返回`EFAULT`、不发布本次fd且不回队已消费bytes/rights。peek对应fault不改变queued original。
- receiver fd numbers先作为reservation写入control，全部output copy成功后才不可失败地publication；它们只在`recvmsg`
  成功返回后承诺可用。并发线程在syscall返回前窥视共享control buffer并抢先使用fd number不属于R0保证；R0不为这一偏僻
  side effect改成Linux式先publication、copy fault后可能遗留partial fd installation。
- 一次receive不会越过第二个rights marker。普通read/recv消费第一个marker时静默释放rights；只有`recvmsg`可以通过
  `MSG_CTRUNC`报告未取得control。

### Unsupported 与诊断

其它Socket family携带send control、Unix seqpacket携带rights、其它ancillary type以及传递AF_UNIX Socket fd均返回
`EOPNOTSUPP`，且拒绝发生在payload copy/family commit前。稳定的target-scope拒绝使用边界限频notice，不在hot path逐次刷屏；
malformed input、普通`EBADF/EFAULT/EAGAIN`和receiver truncation不要求噪声日志。

## Contract Impact

下表保存`UNIX-SCM-RIGHTS-CUTOVER`已经生效的delta；“当前规则”列是cutover前的effective baseline。

| Contract ID | 变化 | 当前规则 | Target 摘要 | Cutover |
| --- | --- | --- | --- | --- |
| [`OPENED-DESC-001`](../../contracts/task/opened-description-lifecycle.md#opened-desc-001--semantic-refcount-是-final-release-的唯一真相) | Refine | semantic lifetime只由published fd slot references决定 | lifecycle owner统一核算published slots与move-only transfer references；最后一种semantic reference消失才terminal | `UNIX-SCM-RIGHTS-CUTOVER` |
| `OPENED-DESC-TRANSFER-001` | Introduce | None | exact fd capture、peek duplication、receiver conversion与abort由`task::files`提供opaque transfer capability；不暴露`ProcFile`/table lock | `UNIX-SCM-RIGHTS-CUTOVER` |
| [`OPENED-DESC-003`](../../contracts/task/opened-description-lifecycle.md#opened-desc-003--当前-final-release-callback-是创建时固定的单-hook) | Refine | 最后published reference移除后运行static hook | 最后published或transfer semantic reference移除后、owner guards外运行同一static hook；不增加dynamic observer | `UNIX-SCM-RIGHTS-CUTOVER` |
| [`OPENED-DESC-RETIRE-001`](../../contracts/task/opened-description-lifecycle.md#opened-desc-retire-001--terminal-retirement-固定进入窄-vfs-flock-handoff) | Refine | 最后published reference触发flock retirement与final-release | terminal handoff延后到最后semantic reference；transfer排队/abort/install不重复或跳过flock cleanup | `UNIX-SCM-RIGHTS-CUTOVER` |
| [`SOCKET-ABI-001`](../../contracts/socket/front-abi-wait.md#socket-abi-001--linux-abi止于family-neutral-adapter) | Refine | Unix stream未发布message I/O；nonzero send control通常拒绝 | adapter解析/编码SCM_RIGHTS、CLOEXEC/CTRUNC/copy ordering并经static capability分发，family只见typed bundle/outcome | `UNIX-SCM-RIGHTS-CUTOVER` |
| [`UNIX-SOCKET-STREAM-001`](../../contracts/socket/unix-stream-lifecycle.md#unix-socket-stream-001--direction-owner提交stream与shutdown结果) | Refine | direction只提交byte prefix、capacity与terminal | direction把rights与首个positive byte position共同commit/consume/peek，并用operation-specific predicate约束rights capacity | `UNIX-SCM-RIGHTS-CUTOVER` |
| `UNIX-SOCKET-RIGHTS-001` | Introduce | None（fd passing明确不在current scope） | Unix stream direction唯一拥有ordered rights markers/charge；Socket ABI与`task::files`按单向handoff完成capture、detach、install和discard | `UNIX-SCM-RIGHTS-CUTOVER` |
| [`UNIX-SOCKET-LIFECYCLE-001`](../../contracts/socket/unix-stream-lifecycle.md#unix-socket-lifecycle-001--publicationhandoff与retirement只有一个cleanup-owner) | Refine | final release先撤销endpoint publication，再由direction提交terminal/cleanup；buffered payload先于terminal结果交付 | `SHUT_RD`只关闭未来ingress；association撤销后的stream/seqpacket retirement detach本端不可达inbound payload、保留peer可读outbound，rights随payload在guard外释放 | `UNIX-SCM-RIGHTS-CUTOVER` |

### Dependencies

- [`SOCKET-FRONT-001`](../../contracts/socket/front-abi-wait.md#socket-front-001--general-front只拥有共同外壳)：static
  descriptor继续是唯一semantic type/capability witness；本RFC不建立Socket core runtime owner。
- [`SOCKET-WAIT-001`](../../contracts/socket/front-abi-wait.md#socket-wait-001--operation-predicate由各自owner定义)：
  request-sized rights capacity使用operation-specific predicate与snapshot/register/recheck，不改变public readiness truth。
- [`OPENED-DESC-002`](../../contracts/task/opened-description-lifecycle.md#opened-desc-002--dupfork-共享-descriptionfd-table-只拥有-publication)：
  receiver fd与sender/fork/dup aliases共享同一opened description；fd table仍只拥有slot publication。
- [`OPENED-DESC-LIVENESS-001`](../../contracts/task/opened-description-lifecycle.md#opened-desc-liveness-001--non-owning-capability-只验证-terminal-opened-description-liveness)：
  既有non-owning capability与operation-local lease仍不延迟retirement；transfer reference是独立、显式的semantic holder，
  不得借用或强化lease。
- [`UNIX-SOCKET-STATE-001`](../../contracts/socket/unix-stream-lifecycle.md#unix-socket-state-001--rolelistener与direction各有唯一truth)：
  endpoint role与direction facts继续分离，rights marker不进入endpoint或listener。
- [`UNIX-SOCKET-ADDRESS-001`](../../contracts/socket/unix-stream-lifecycle.md#unix-socket-address-001--address-snapshot独立于namespace-lifetime)：
  `recvmsg` name output只投影connection已有peer-name snapshot，不增加pathname lookup或新的address state。
- [`UNIX-SOCKET-SEQPACKET-001`](../../contracts/socket/unix-seqpacket.md#unix-socket-seqpacket-001--record-transaction由direction-owner一次提交)：
  record transaction、ABI与readiness保持不变；seqpacket只与stream共同修复既有endpoint-retirement inbound detach，不获得
  rights marker、message control或新的contract delta。
- [`KUNIT-EXEC` / `KUNIT-CONCURRENCY` / `KUNIT-PROOF` / `KUNIT-SHAPE`](../../contracts/kunit/execution-and-proof.md)：
  KUnit执行、并发准入、证明外推、cleanup和production shape。

## Implementation Boundary

- **允许改变：** native network UAPI中的SCM_RIGHTS/flags/constants；Socket message ABI parser/encoder、static capability与Unix
  stream message dispatch；`task::files` opened-description lifecycle/transfer/reservation owner API；Unix stream
  direction的ordered rights association、capacity/wait/drain；stream与seqpacket既有direction的final-release inbound detach；
  相关Kconfig、owner-local inline KUnit、
  `anemone-apps/socket-test` Unix定向suite；最终cutover所需current contract、register和公共导航。同owner新文件、
  import/re-export、模块注册及行为保持型拆分可自然闭合。
- **必须保持：** R0 rights transport只覆盖`AF_UNIX + SOCK_STREAM`、拒绝传递任何AF_UNIX Socket fd、无cycle GC、static
  capability absence、opened-description单一semantic lifecycle、byte/rights共同direction commit、receive单向detach、
  all-or-none receiver fd publication、guard外drop/notify、bounded capacity、本文errno/copy/peek/truncation语义、唯一cutover
  与验证下限。`SHUT_RD`保留已经排队的stream/seqpacket payload，final release只detach本端不可达inbound并保留peer可读
  outbound；seqpacket ABI、record transaction与readiness保持不变。current contract在cutover前保持不变。
- **实现自由：** rights position可用monotonic stream sequence、ordered segment或其它direction-local表示；transfer type、
  lifecycle word/ref representation、install plan、operation wait carrier、module/file split与helper名称由实现选择。任何表示都不能
  缓存第二份readiness/alive truth、让marker脱离byte identity、向Unix暴露`ProcFile`/Task/table lock，或使final release依赖
  Rust storage drop。shutdown/retirement的direction-local transition与resource carrier形状由实现选择，但endpoint association
  必须继续是consumer reachability唯一truth。
- **工程妥协：** target外的偏僻Linux side effect、mixed-invalid winner、公平性、逐字节同形、allocation-free纯度或需要
  通用ancillary/GC框架才能获得的完整性，在不造成ABI谎言、引用泄漏、双重publication、第二份truth或主路径错误时，默认以
  显式unsupported、Not Proven或cutover后的accepted limitation记录，不因此阻塞R0。target内错误仍是open issue或blocker。
- **停止条件：** 若实现需要允许AF_UNIX fd、引入inflight cycle/GC、移动ABI或lifecycle owner、为retirement增加第二份
  consumer reachability状态、改变seqpacket ABI/record semantics、在Unix guard内操作fd table/
  VFS、建立generic ancillary bag、不能用单一semantic reference model闭合sender-close/abort/install、需要requeue已detach
  rights、改变本文copy/peek/truncation/errno、降低双架构validation、发布partial ABI、引入probe/不安全中间态/多个cutover，
  或发现本轮无法neutralize的Apollyon/Keter，则在cutover前停止并回到RFC review或Target Renegotiation。

本R0默认使用一个连续implementation unit与唯一`UNIX-SCM-RIGHTS-CUTOVER`。普通commit、同owner结构整理和review slice不
构成独立gate；除非后续证据命中上述停止条件，不创建`implementation.md`、`tracking-issues.md`、transaction或额外stage。
RFC接受只固定target和Implementation Boundary，不自动授权实现；实现授权由维护者另行给出。

## Acceptance 与 Validation

本R0接受target、owner/handoff、ABI差异、工程妥协与验证下限；接受本身不会发布能力或修改current contract。
最终closure与cutover必须同时满足以下证据。

### 源码审查

- 审计Socket front/static descriptor，确认raw cmsg、user pointer、flags、errno与copy ordering不进入Unix或`task::files`，
  unsupported family不出现runtime registry、generic bag或private downcast。
- 审计全部fd capture/install/abort路径与opened-description release caller，确认published/transfer references同源、exact
  capture不受fd reuse影响、conversion无terminal gap、sender close后receiver可用、flock retirement与static final-release
  exactly once且不在fd-table guard内运行。
- 审计Unix send/receive/read/shutdown/final-release，确认bytes与marker共同commit/detach、一次receive至多一个group、
  partial/zero/peek/ordinary discard有唯一语义、rights charge/wait/notify同源，并且所有bundle drop在owner guard外。
- 审计stream/seqpacket endpoint association、direction与operation gate，确认`SHUT_RD`保留queued payload而只关闭未来ingress；
  final release先撤association再detach本端inbound、保留peer可读outbound；staged receive在commit前重检association，peer send与
  retirement由connection owner排序，listener child与connection destruction没有遗留不可达payload。
- 审计`sendmmsg`逐项wrapper，确认partial positive element写入实际`msg_len`后停止，send failure与`msg_len` copyout fault保持
  既有completed-count/fail-forward语义，已提交rights bundle不requeue、不跨element重放。
- 对照固定Linux 6.6.32的`scm.c`与`af_unix.c`确认本文明确支持和明确偏离的行为，没有把Linux internal GC或skb shape误写成
  Anemone target。

### Owner-local KUnit

- `task::files` inline KUnit覆盖exact capture、multi-fd all-or-none abort、transfer duplication、sender last-close保活、receiver
  reservation/conversion、CLOEXEC、fd reuse、install/drop竞争模型与最后semantic reference触发一次retirement。
- Unix stream owner inline KUnit覆盖marker顺序与byte position、partial positive/zero send、per-message/direction capacity、
  operation-specific wait predicate、nonpeek detach、普通read discard、重复peek、短prefix不提前取得rights、两个marker短读、
  `SHUT_RD`保留queued bytes/rights且阻断未来ingress、endpoint retirement detach本端inbound并保留peer可读outbound，以及
  guard外cleanup carrier。
- Unix seqpacket owner inline KUnit覆盖`SHUT_RD`保留queued record、final release detach本端inbound/preserve peer-readable
  outbound和byte/record accounting归零；不增加rights marker或测试专用production seam。
- Socket message ABI inline KUnit覆盖native cmsg alignment/overflow、multiple/empty rights cmsg、bad fd/AF_UNIX reject、flags、
  name/control/header ordering、short/absent control、fd ceiling、`MSG_CTRUNC`、`MSG_CMSG_CLOEXEC`、pre/post-commit copy fault，
  以及`sendmmsg` partial element、`msg_len` copyout fault与bundle exactly-once。
- 测试遵循KUnit current contract；除非live scheduling/wait本身是被测语义，不用yield/sleep模拟happens-before，不为本组测试
  新建默认`kunit.rs/tests.rs`或production probe facade。

### 双架构定向 `socket-test`

- RV64与LA64 release guest均运行长期`socket-test` Unix SCM_RIGHTS suite，覆盖socketpair与pathname connection、无control
  `sendmsg/recvmsg`、单个/多个cmsg和多个fd、regular file shared offset/status、pipe与一个非Unix Socket fd、sender close后
  receiver继续使用、dup/fork共享及receiver close cleanup。
- 覆盖sender CLOEXEC不继承、`MSG_CMSG_CLOEXEC`经exec child验证、bad/mixed fd、AF_UNIX stream/seqpacket fd整包拒绝、
  zero payload不交付、per-message上限、nonblocking capacity `EAGAIN`与consume后可重试。
- 覆盖无/短control、部分容纳、fd-table exhaustion、payload fault、name/control/header fault、普通read跨marker丢弃、
  `MSG_PEEK`重复安装、peek后普通consume；`SHUT_RD`后仍以`recvmsg`取得queued bytes/rights且peer新send得到`EPIPE`；ordinary
  read discard、endpoint final close与listener-child retirement通过pipe EOF证明不可达reference释放。每项同时检查byte order、
  `MSG_CTRUNC`、fd可用性与未安装reference最终释放。
- 覆盖`sendmmsg`携带rights的多element顺序、partial positive stream element停止与实际`msg_len`、首项/后续项failure，以及
  `msg_len` copyout fault后的payload/rights单次commit；未处理element的control不得被capture或重放。
- 至少一次RV64 `smp=4` focused run覆盖共享socket的send/recv/peek/close竞争；它只能证明targeted interleaving与cleanup，
  不外推穷尽调度公平性。

### 构建、文档与 Not Run

- 通过repository entrypoint完成kernel、`socket-test`与相关app格式/构建，RV64/LA64 release kernel及默认Kconfig关系校验；
  `git diff --check`与`mdbook build docs`通过。
- full Socket/Network LTP、final harness、physical hardware、LA64 `smp>1`、其它SMP拓扑、long pressure、Unix datagram/
  seqpacket rights、compat32、完整ancillary与cycle GC均为Not Run / outside R0，不能从上述证据外推。

## 风险与反馈

最高风险是把transfer reference误写成第二份opened-description lifetime、在last published slot close时提前触发flock/
Socket final release，或为control copy rollback让fd table与Unix direction互相持锁。第二风险是marker/byte position在partial
send、ordinary read、peek与shutdown中漂移，导致rights重复、提前交付或永久滞留。第三风险是把`SHUT_RD`误当consumer
retirement、或在final release保留已经不可达的inbound payload。实现应优先保持本页单向handoff：
`task::files capture -> Unix direction queue -> ABI detach/peek bundle -> task::files install/drop`，不增加反向callback或requeue。

在accepted target内，marker容器、transfer representation、模块拆分和验证case可按live source自然调整。若证据要求改变
Unix-fd拒绝、owner、receive fail-forward、ABI、capacity guarantee、contract delta或validation claim，必须在cutover前回到
RFC review；agent可以提交成本和reduced-target证据，不能自行把较弱能力写成R0完成。

## 文档与证据

- current baseline：
  [Socket front](../../contracts/socket/front-abi-wait.md)、
  [Unix stream lifecycle](../../contracts/socket/unix-stream-lifecycle.md)、
  [Unix seqpacket](../../contracts/socket/unix-seqpacket.md)、
  [opened-description lifecycle](../../contracts/task/opened-description-lifecycle.md)。
- live source：`anemone-kernel/src/fs/socket/api/message/`、
  `anemone-kernel/src/fs/socket/api/profile.rs`、
  `anemone-kernel/src/fs/socket/front/`、
  `anemone-kernel/src/fs/socket/unix/endpoint/mod.rs`、
  `anemone-kernel/src/fs/socket/unix/endpoint/stream.rs`、
  `anemone-kernel/src/fs/socket/unix/endpoint/record.rs`、
  `anemone-kernel/src/task/files/`。
- 外部源码证据：`xref:linux-6.6.32:net/core/scm.c#scm_fp_copy`、
  `xref:linux-6.6.32:net/core/scm.c#scm_detach_fds`、
  `xref:linux-6.6.32:net/socket.c#__sys_sendmmsg`、
  `xref:linux-6.6.32:net/unix/af_unix.c#unix_shutdown`、
  `xref:linux-6.6.32:net/unix/af_unix.c#unix_release_sock`、
  `xref:linux-6.6.32:net/unix/af_unix.c#unix_stream_sendmsg`、
  `xref:linux-6.6.32:net/unix/af_unix.c#unix_stream_read_generic`。
- commit：`scm-rights: implement Unix stream fd passing`；PR / optional transaction：None。

## 修订记录

- **R0（2026-08-17，Accepted）：** 接受connected native `AF_UNIX + SOCK_STREAM`的`sendmsg/recvmsg + SCM_RIGHTS`、
  opened-description transfer lifecycle、byte/rights共同transaction、stream/seqpacket endpoint-retirement prerequisite、
  单一cutover与验证矩阵。review将`SHUT_RD`固定为只关闭未来ingress并保留queued payload，final release固定为association撤销后
  detach本端不可达inbound/preserve peer-readable outbound；同时补齐`sendmmsg` partial、`msg_len` copyout fault与bundle
  exactly-once proof。接受未发布能力、未修改current contract，也未授权越过本页Implementation Boundary。

## Closure

Closed R0。唯一`UNIX-SCM-RIGHTS-CUTOVER`原子交付native connected Unix stream `sendmsg/recvmsg + SCM_RIGHTS`、
opened-description transfer lifecycle、ordered byte/rights transaction、bounded request-sized wait，以及stream/seqpacket
endpoint-retirement inbound detach；current contracts已同步Introduce/Refine上述八个ID。register审计没有发现target内open
issue或target外accepted limitation，因此没有新增register处置；未创建supporting page、transaction或后续gate。

源码审查覆盖Socket ABI/static capability、exact capture与all-or-none install、semantic final release、stream marker/wait、
stream/seqpacket retirement、guard外cleanup及`sendmmsg` fail-forward。独立review最初发现Socket message owner-local KUnit与
三个guest edge case覆盖不足这一项Keter；补齐capture/reservation/publication/copy-ordering/fail-forward KUnit及短control、
首项send failure、已完成元素后的`msg_len` fault guest oracle后，复核结果为Apollyon 0、Keter 0、Euclid 0。

agent运行的当前源码证据：RV64 release kernel、LA64 release kernel及双架构`socket-test` build通过；RV64 SMP1与SMP4均
797/797 KUnit且完整guest suite输出`SCMRIGHTSTST:PASS`，LA64 SMP1为801/801 KUnit且同一suite PASS。RV64两次均orderly
poweroff；LA64完成marker后进入平台既有“no power off handler succeeded”halt，随后只退出QEMU monitor，不把host退出方式
外推为guest失败。`just fmt kernel --check`、`just fmt socket-test --check`、`git diff --check`与`mdbook build docs`在最终
closure检查通过。

full Socket/Network LTP、final harness、physical hardware、LA64 `smp>1`、其它SMP拓扑、long pressure、Unix datagram/
seqpacket rights、compat32、完整ancillary与cycle GC均Not Run / outside R0，不从本次focused证据外推。
