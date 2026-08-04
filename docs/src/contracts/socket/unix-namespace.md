# Unix Socket Pathname Namespace 当前契约

**Contract ID：** `UNIX-SOCKET-NAMESPACE-001`
**状态：** Active
**Owner：** VFS拥有pathname resolution、create/DAC/inode identity与link lifecycle；Unix namespace拥有exact inode identity到live binding capability的association
**参与领域：** Unix IPC / VFS creation / filesystem backend / credentials / Socket lifecycle
**覆盖范围：** filesystem pathname stream/seqpacket bind、connect lookup/DAC、profile-compatible admission、inode-identity registration、unlink/hard-link/rename/rebind与retirement isolation
**不覆盖：** abstract namespace、autobind、socket-local umask/VFS policy、generic inode attachment或cross-owner rollback framework
**实现位置：** `anemone-kernel/src/fs/socket/unix/{namespace.rs,endpoint/mod.rs}`、`anemone-kernel/src/fs/socket/api/{bind,connect}.rs`、`anemone-kernel/src/fs/api/creation.rs`
**依赖：** `VFS-FILE-KIND-001`、`VFS-CREATION-001`、`VFS-MAKE-NODE-001`、`SOCKET-FRONT-001`、`SOCKET-ABI-001`、`UNIX-SOCKET-STATE-001`、`UNIX-SOCKET-ADDRESS-001`、`UNIX-SOCKET-LIFECYCLE-001`
**Pending Successor：** None
**最后核验：** 2026-08-04

## 状态与能力所有权

| 状态 / 能力 | 唯一 Owner | 其它参与方持有什么 | 行为用途 |
| --- | --- | --- | --- |
| pathname resolution、parent create admission、umask/credential snapshot与final inode metadata | task filesystem context / VFS creation operation / filesystem backend | Unix bind只提交socket kind与requested permission | filesystem-backed Socket inode creation |
| inode kind、identity与link/rename/unlink lifecycle | VFS inode/dentry owner | Unix namespace以stable `InodeRef` identity作为key | lookup与stale isolation |
| live binding association与generation | Unix namespace | entry持endpoint weak capability；endpoint持registration authority | connect解析与exact retirement |
| immutable connection profile | `UnixEndpointCore` | namespace只经窄admission capability请求compatible profile | profile-compatible connect admission与connection construction |
| local/peer Linux-visible pathname | endpoint address snapshot | namespace不修改snapshot | address query，见`UNIX-SOCKET-ADDRESS-001` |

## UNIX-SOCKET-NAMESPACE-001 — VFS identity与live binding分属两个owner

**规则：** pathname bind先让endpoint取得bind preparation authority，再通过user-thread VFS creation operation提交`S_IFSOCK`与`0777` requested permission。task filesystem context唯一提供process umask，operation-local credential/mask snapshot完成parent search/create admission与final metadata formation，context-free VFS primitive及backend拥有inode/dentry publication。Unix owner不得复制umask、DAC、pathname traversal或inode kind policy。

VFS创建成功后，Unix namespace以resident `InodeRef`的exact identity注册到endpoint weak capability，并产生generation-scoped registration；endpoint只有在registration仍current时commit local binding。connect每次重新经VFS解析目标、验证Socket inode kind与target `WRITE` DAC，再按exact identity取得与client profile兼容的窄admission capability；blocking retry不得复用旧DAC结果、inode lookup或binding capability。stream client连接seqpacket listener或反向连接时，Unix owner必须在backlog reservation、connection/accepted-child publication与client role commit前返回`EPROTOTYPE`。profile只服务Unix owner内的admission与connection construction，不能替代Socket descriptor成为UAPI type truth。

hard link/rename只改变VFS可达pathname，所有指向同一resident inode identity的路径解析到同一live binding。unlink不自动retire live endpoint或既有connection；原inode失去链接后，新create同名pathname得到新identity/generation，旧registration cleanup只能移除自己，不能命中新binding。namespace entry不强持endpoint，retired/expired weak capability fail closed。

**Failure / cleanup：** VFS create失败时endpoint撤销bind preparation且不发布name/registration。若inode已由VFS成功发布、但之后Unix registration或endpoint commit失败，首版允许留下不关联live endpoint的inert Socket inode；该可见限制由register拥有。不得为消除该退路跨VFS operation持Unix global lock、建立第二VFS truth或新增generic rollback framework。

**违反表现：** pathname-keyedruntime registry；inode `prv`承载live endpoint；Socket自己应用umask/DAC；connect retry缓存授权；cross-profile connect在backlog或role commit后才失败；namespace profile反向驱动front query；unlink销毁live binding；old generation移除new binding；namespace强持endpoint；失败后半发布local name。

**验证 / Enforcement：** exact identity/generation与profile mismatch KUnit；production VFS `0777 -> umask 0027 -> 0750` handoff、parent/target DAC、cross-profile `EPROTOTYPE`、hard-link/rename/unlink/rebind、retired endpoint与stale cleanup source/guest matrix。

**最初来源：** [Socket Abstraction 与 Unix Socket RFC R1](../../rfcs/socket-abstraction-and-unix-socket/index.md)。

**当前来源：** 同RFC Stage 4 `SOCKET-UNIX-CUTOVER`建立baseline；[Unix seqpacket小迭代](../../devlog/changes/2026-08-04-unix-seqpacket.md)的`SOCKET-UNIX-SEQPACKET-CUTOVER`加入profile-compatible admission与cross-profile `EPROTOTYPE`。

## 当前接受边界

- 当前pathname representation要求可接受的UTF-8 path；non-UTF-8差异由register记录。
- retired bind留下inert inode与VFS create publication atomicity问题保持既有owner和register边界；本contract不声称bind跨VFS/Unix owner完全原子。
- closure evidence覆盖RV64/LA64真实guest与RV64 `smp=4` focused runtime；physical hardware、LA64 `smp>1`、其它SMP拓扑、full socket/network LTP与final harness Not Run。
