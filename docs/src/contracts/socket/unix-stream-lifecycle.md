# Unix Socket State、Stream、Address 与 Lifecycle 当前契约

**Contract IDs：** `UNIX-SOCKET-STATE-001`、`UNIX-SOCKET-STREAM-001`、`UNIX-SOCKET-ADDRESS-001`、`UNIX-SOCKET-LIFECYCLE-001`
**状态：** Active
**Owner：** Unix endpoint role/association、listener/backlog、connection directional stream与immutable address snapshot各自拥有对应state；本页拥有它们之间的handoff协议
**参与领域：** Unix IPC / Socket front / VFS namespace / opened description / iomux / epoll
**覆盖范围：** socketpair与pathname stream的role、listen/connect/accept、byte stream、shutdown/EOF/readiness、address snapshot及final-release lifecycle
**不覆盖：** Unix datagram/seqpacket/abstract namespace、credentials/fd passing、ancillary data、autobind、pre-connection shutdown persistence、pending error/`SO_ERROR`
**实现位置：** `anemone-kernel/src/fs/socket/unix/{admission.rs,endpoint/}`、`anemone-kernel/src/fs/socket/unix/endpoint/stream.rs`、`anemone-kernel/src/fs/socket/api/`
**依赖：** `SOCKET-FRONT-001`、`SOCKET-ABI-001`、`SOCKET-WAIT-001`、`UNIX-SOCKET-NAMESPACE-001`、`OPENED-DESC-001..003`、`IOMUX-POLL-001..003`、`EPOLL-WATCH-001`、`EPOLL-READY-001`
**Pending Successor：** None
**最后核验：** 2026-08-02

## 状态与能力所有权

| 状态 / 能力 | 唯一 Owner | 其它参与方持有什么 | 行为用途 |
| --- | --- | --- | --- |
| endpoint role与active association | `UnixEndpointCore` | operation取得role-scoped capability/snapshot | bind/listen/connect/accept dispatch与retirement |
| backlog、queued accepted child与connect/accept predicate | `UnixListener` | endpoint持listener association；waiter持non-owning route | admission、capacity与listener close |
| peer relation与两条direction | `UnixConnection` | endpoint只持side | connected stream routing |
| bytes、capacity、writer/reader terminal与routes | 对应directional stream | send/receive/poll取得operation-local access | prefix commit、EOF、RDHUP/HUP与wake |
| Linux-visible local/peer pathname | endpoint的immutable address snapshot | namespace只拥有live inode-to-binding association | name query与close/unlink后的visibility |

## UNIX-SOCKET-STATE-001 — Role、listener与direction各有唯一truth

**规则：** endpoint唯一拥有当前unconnected/bound/listening/connected/retired role与对应association。listener唯一拥有backlog、pending accepted child queue及connect/accept capacity；connection唯一配对两个endpoint side；每条direction唯一拥有bytes、capacity与writer/reader terminal facts。endpoint不得复制listener count、direction terminal或ready mask。

role transition在旧owner撤销publication后才把能力交给新owner；pre-listen与pre-connect route随handoff迁移到listener或connection predicate。route只携带recheck capability，不反向驱动role、admission或stream state。

**违反表现：** endpoint和direction双写shutdown/terminal；两份backlog count或admission queue；role tag与optional state可形成不一致组合；notification carrier成为behavior truth；retired endpoint仍发布route或接受operation。

**验证 / Enforcement：** role transition、backlog resize/close、admission、route handoff、direction combination与retirement KUnit；source/lock-order audit；两架构listen/connect/accept/readiness runtime。

**最初来源：** [Socket Abstraction 与 Unix Socket RFC R1](../../rfcs/socket-abstraction-and-unix-socket/index.md)。

**当前来源：** 同RFC Stage 4 `SOCKET-UNIX-CUTOVER`与Git/PR closure evidence。

## UNIX-SOCKET-STREAM-001 — Direction owner提交stream与shutdown结果

**规则：** `socketpair`或connect/accept admission建立一份paired connection与两条bounded direction。send只向peer inbound direction staged-copy，并在commit前重检endpoint/connection仍current；receive从local inbound direction staged-copy，普通receive只消费已复制prefix，`MSG_PEEK`不消费bytes或capacity。zero-length、partial progress和copy fault按已成功prefix映射，不能持spinlock执行user copy。

writer shutdown/final close在对应direction提交peer EOF；reader shutdown使后续local receive terminal并让peer send得到`EPIPE`，除`MSG_NOSIGNAL`外发布`SIGPIPE`。buffered bytes先于EOF交付。receive-half-close、完整HUP和ordinary readability/writability都从direction current facts投影；RDHUP与HUP是独立事实。

首版只支持connected shutdown。unconnected、bound或listening role对三个合法`how`都返回`ENOTCONN`，不保存pending intent且不影响后续listen/connect/accept；该Linux差异由register拥有。

**Failure / cleanup：** staged copy只在operation gate与current-association recheck通过后commit，terminal/retired transition后不得提交旧prefix。normal capacity不足返回not-ready并进入共同wait协议，不用无界queue、panic或busy-poll吸收。

**违反表现：** copy fault消费未报告bytes；peek释放capacity；endpoint保存第二份terminal；shutdown后旧copy仍commit；EOF越过buffered prefix；RDHUP等同完整HUP；pre-connection shutdown静默成功并保存hidden state。

**验证 / Enforcement：** direction order/capacity、partial/copy-fault、peek、shutdown、SIGPIPE、EOF、readiness与register-recheck KUnit；glibc/musl focused oracle；RV64/LA64 socketpair与pathname stream I/O/runtime。

**最初来源：** [Socket Abstraction 与 Unix Socket RFC R1](../../rfcs/socket-abstraction-and-unix-socket/index.md)。

**当前来源：** 同RFC Stage 4 `SOCKET-UNIX-CUTOVER`与Git/PR closure evidence。

## UNIX-SOCKET-ADDRESS-001 — Address snapshot独立于namespace lifetime

**规则：** 成功bind在endpoint保存一份immutable local pathname snapshot；connect/accept在connection admission时取得peer-name snapshot。`getsockname`/`getpeername`只读取这些snapshot，不以当前VFS lookup或live binding为truth。rename、unlink、binding retirement与peer final close不改写已经建立的address snapshot；未命名endpoint保持明确的unnamed结果。

connected unnamed Socket允许在首版范围内随后pathname bind一次；accepted endpoint继承listener local-name语义，不能再次bind。snapshot不授予namespace admission或connect authority。

**违反表现：** query按pathname重新lookup；unlink/rename清空name；peer close丢失peer address；namespace key与Linux-visible address共用一份mutable truth；address snapshot被用于绕过DAC或live-binding检查。

**验证 / Enforcement：** connected-later bind、accepted-name、peer-close visibility、rename/unlink/rebind与sockaddr output KUnit/guest matrix。

**最初来源：** [Socket Abstraction 与 Unix Socket RFC R1](../../rfcs/socket-abstraction-and-unix-socket/index.md)。

**当前来源：** 同RFC Stage 4 `SOCKET-UNIX-CUTOVER`与Git/PR closure evidence。

## UNIX-SOCKET-LIFECYCLE-001 — Publication、handoff与retirement只有一个cleanup owner

**规则：** socketpair先保留两个fd slot；fd-pair copyout后再准备两端unpublished state，任一pre-publication failure撤销reservation及已准备state；两次infallible commit后不再返回失败。connect只在listener capacity、current binding/DAC和client role recheck后把paired child交给listener queue；accept先detach一个child，再完成accepted fd preparation/copyout/publication，失败由当前transaction清理且不重复入队。

semantic final release先撤销endpoint operation/route/binding publication，再由listener或connection/direction owner提交queued-child withdrawal、EOF/write failure和route snapshot，最后在guard外notify/drop。关闭非最后alias不推进这些事实。unlink只撤销VFS link，live binding与既有connection可继续；binding retirement按exact inode identity和generation撤销，旧registration或late route不得命中新generation。

listener close撤销新admission并drain未接受child；unpublished preparation的Drop只负责abort尚未handoff的authority，不能与live endpoint final release形成双重cleanup。

**违反表现：** partial fd publication可返回失败；accept failure泄漏child/fd或重复入队；final close后仍可admit；旧binding cleanup移除新generation；一个dup close提前EOF；guard内notify/drop final reference；Drop与explicit retire都拥有同一live cleanup。

**验证 / Enforcement：** pair/accept rollback、listener drain、dup/fork/final close、unlink/rebind/stale generation、late hint与unpublished abort KUnit/host/guest matrix；完整handoff source audit。

**最初来源：** [Socket Abstraction 与 Unix Socket RFC R1](../../rfcs/socket-abstraction-and-unix-socket/index.md)。

**当前来源：** 同RFC Stage 4 `SOCKET-UNIX-CUTOVER`与Git/PR closure evidence。

## 当前接受边界

- 当前成功面是pathname `AF_UNIX + SOCK_STREAM`及connected unnamed socketpair；abstract namespace、autobind、datagram/seqpacket、credentials/fd passing与ancillary data均不由本页推出。
- non-UTF-8 pathname、retired bind留下inert inode及pre-connection shutdown差异由register记录；VFS common-create publication问题仍由VFS owner拥有。
- closure evidence覆盖RV64/LA64真实guest和focused libc/owner proof。physical hardware、`smp>1`、full socket/network LTP与final harness Not Run。
