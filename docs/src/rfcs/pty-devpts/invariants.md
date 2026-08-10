# PTY / devpts 目标与不变量

**状态：** Accepted Target
**父 RFC：** [RFC-20260810-pty-devpts](./index.md)
**适用修订：** R1
**最后更新：** 2026-08-11

本页只展开父 RFC 已提出的 non-trivial correctness 和 contract proof obligations，不增加 target、ABI、stage 或
implementation authorization。slave initial metadata、route-scoped permission profile、implicit controlling-terminal
acquisition、Linux-default hangup surface、master-hangup relation effect、safe-reuse resource guarantee、final-release
composition boundary 与 claim-scoped acceptance 均以父 RFC R1 为准。

## PTY-IDENTITY-001 — Pair、terminal 与 pathname identity 不得跨 episode 复活

每次成功 `/dev/ptmx` allocation 形成 immutable pair identity 与 slave semantic-terminal identity。master/slave file、
devpts backend binding、slave inode/open capability、relation handle 和 waiter registration 可以持该 identity 的窄
capability或snapshot，但不能用 index、inode number、device number、raw pointer 或可升级 weak reference单独判断
pair live。VFS dentry只投影其持有的inode，不取得pair liveness或numeric-index rebinding authority。

master final close 提交 retirement 后，该 pair identity 永不重新进入 live。若 index 复用，新的 pair 必须具有由
devpts live binding、slave inode/open capability 与 pair admission共同核验的新 identity/incarnation；旧 inode、handle
或 operation snapshot 永远不能接入新 pair。R1 要求 current configured capacity 内 repeated allocate-close 不因历史
churn 累计耗尽。generic VFS pathname freshness/revocation 缺口继续由对应 register issue拥有，不是 PTY
implementation/cutover Stage；devpts 不得读取 VFS private cache、建立 owner-local dentry freshness truth，或退成
monotonic boot-lifetime exhaustion。

**违反表现：** stale `/dev/pts/N` open 命中新 pair、cached inode 通过 reused index 重新获得 liveness、pair 从
retired 回到 live，或 numeric `N` 成为唯一 pair identity。

**证明面：** identity type/source audit；retire-then-reallocate、旧 inode/capability fail-close、新 binding 指向新 pair
与 operation snapshot revalidation；capacity 内跨越编号空间的 repeated churn、allocator exhaustion/reuse测试。
generic cached-positive、late materialization 与 multi-view pathname linearizability 继续由 VFS register 记录为 Not
Proven，不由本不变量冒充关闭。

## PTY-ALLOC-001 — Allocation 是 prepare-before-publish 的单一 episode

system devpts instance 是 `/dev/ptmx` allocation transaction owner。它从 allocator task 取得一次 operation-local
`fsuid`/`fsgid` snapshot，可以编排 index reservation、prepared pair、master opened description、slave
metadata/binding 与 VFS publication，但不得读取或推进 pair runtime state。

initial slave inode 必须在 publication 前一次形成 allocator `fsuid:fsgid`、mode `0600`、character kind 与
`st_rdev=136:N`；allocator umask 不参与该 fixed devpts profile。devpts 只向 VFS 提交这些 initial metadata facts，
resident uid/gid/mode/rdev 仍由 VFS inode metadata owner 保存和投影，不在 pair 中缓存第二份行为 truth。

成功返回 master fd 前，pair、master description 与 locked slave binding 必须全部可形成自洽 episode。所有 fallible
prepare 发生在 visibility commit 前；commit 后不得再存在需要通过撤销已返回 master fd 或留下 half-published node
恢复的普通失败。失败 cleanup 只回收本次未发布 capability和reservation，不影响其它 live pair。

fd publication仍服从 `task::files` reservation/commit truth；backend binding publication、VFS dentry materialization 与
fd slot publication不需要共享一个 global lock，但必须有明确的先后和失败边界，使任何 concurrent observer 只看到
不可发现前态或完整成功 episode。

**违反表现：** master fd 已返回但 slave binding 永远不可完成、slave 可被打开但 master description 未发布、失败留下
live index/pair、cleanup误退休其它 pair，或 allocation transaction长期取得pair lifecycle authority。

**证明面：** 每个 fallible step 的 rollback table；fd reservation abort/commit；duplicate/exhaustion/allocation failure；
concurrent lookup/open 在 publication 前后的可见状态。

## PTY-ADMISSION-001 — Slave open 只有一个仲裁点

pathname open 与 `TIOCGPTPEER` 都必须提交同一个 pair-owned lifecycle admission transaction，但其 route-local
前置条件不同。pathname route 先由 VFS 使用 current opener credential 完成 pathname search 与 inode-mode DAC；
`TIOCGPTPEER` route 必须验证调用 fd 是该 live pair 的 master capability并核验 ioctl flags，不重新执行 pathname
traversal 或 ordinary pathname DAC。两条路线随后都由 pair owner 核验 pair live 与 slave lock，并在 retirement
仲裁点前完整 enrollment 一个新的 slave opened description。

admission commit 与 master retirement 必须线性化：admission先提交时，description已经具备完整 pair participation并
可由后续 final release exactly-once撤销；retirement先提交时，所有新 admission fail closed。失败不得发布 fd slot、
增加 participant、创建 relation 或遗留 waiter。

`TIOCSPTLCK`只修改 pair-owned lock state，不修改 pathname/VFS publication。unlock使后续 admission有资格成功，
但不绕过各 route 自己的前置条件、pair liveness 或 capacity。pathname path 与 peer ioctl 可以有不同 ABI parser和
permission/capability admission，不能形成两套 pair lifecycle 或 enrollment owner。

**违反表现：** pathname route跳过VFS DAC、peer ioctl重新依赖pathname traversal/DAC或接受无效master capability、
locked slave通过另一入口打开、open与retire产生half-enrolled fd、failed fd commit遗留participant，或devpts/VFS
自行裁决pair live。

**证明面：** pathname DAC与peer master-capability route-difference matrix；两条路线的shared opened-description
semantics；locked/unlocked/invalid master/permission/invalid flags；open-vs-retire deterministic interleaving；
publication failure cleanup。

## PTY-DESC-001 — Peer participation 由 opened-description final release 推进

pair的slave participation只表示成功enroll且尚未semantic final release的slave opened descriptions。它不是
`task::files` published-ref truth的副本，也不能从`Arc` strong count、raw fd数量、inode/dentry refs或`File` storage
lifetime反推。

同一description的dup/fork aliases只增加published slots；关闭非最后slot不改变pair participation。多次pathname/
peer open产生不同descriptions，并分别参与。`Live(1) -> Retired`后，`task::files`仍只执行 current
`OPENED-DESC-003` 在creation time固定的单 static hook；PTY open owner必须在该边界内 owner-locally composition pair
participation与既有fanotify close effect。pair处理必须exactly-once或幂等，并且不得访问fd-table private
lock/lifecycle word。

existing flock retirement继续先于该single hook。本 RFC 不为PTY effect与`FAN_CLOSE_*`规定没有用户可见要求的先后，
但两者都不得被覆盖或遗漏；实现不得建立runtime observer registry、让fanotify或pair取得opened-description truth，或在
fd-table write guard内进入外部owner。若无法在current contract内自然组合，必须返回RFC review，不能自行扩大
`task::files` shared surface。

**违反表现：** close一个alias触发peer absence、transient syscall borrow延迟final release、PTY hook覆盖fanotify、
fanotify回调决定pair liveness、final release重复decrement participant，或cleanup持fd-table guard进入TTY/VFS。

**证明面：** dup/fork/close/close-on-exec/table teardown；multiple independent opens；flock-before-hook与
PTY/fanotify两项effect均存在；concurrent final closes；creation/commit rollback。

## PTY-PAIR-001 — Peer absence 与 retirement 是不同状态转换

master live时，last-slave-description final release只把slave participation变为zero并发布peer-absence predicate；它不
retire pair、不锁回slave，也不撤销devpts binding。后续符合policy的slave open可以重新enroll，并让master operation
观察peer重新出现。

master description final release是唯一不可逆 retirement trigger。pair owner在线性化点内禁止新admission并发布
retired/hangup predicate；该转换不等待devpts、relation、Signal或wait-owner执行。retirement后所有handoff都只能推进
monotonic cleanup，不能恢复master/slave live、重新发布binding或撤销已经对operation可见的hangup。

首版支持surface的peer absence与retirement前后buffered data、EOF/`EIO`、write error、query ioctl和poll bits遵循
Linux 6.6.32用户可观察语义；逐格source/test matrix由实现形成proof。无论具体case，blocked operation必须在
predicate变化后最终重验并得到terminal outcome，不能永久sleep或success-no-op；有意偏离必须先回到RFC review。

**违反表现：** last slave close永久删除pair、slave reopen复活已retired pair、master final close等待全局cleanup后才
发布HUP、late cleanup重新使node可open，或wake edge丢失导致waiter永久阻塞。

**证明面：** last-slave-close/reopen、master-first/slave-first close、buffered/unbuffered paths、blocked reader/writer/
poller、partial progress与concurrent close/open。

## PTY-READY-001 — Readiness 来自 owner predicate 的无缓存组合

slave-side stream readiness继续由`Terminal` data availability/capacity拥有；master-side operation把对应Terminal
predicate与pair-owned peer presence/hangup/retirement组合。任何FileOps、poll route、event或waiter都不能保存第三份
readable/writable/HUP truth。

notification只表示predicate可能改变。blocking I/O、poll/select与epoll都必须执行snapshot/register/recheck；取消或
注册失败执行cancel/finish/final snapshot，不用wake count、route generation或one-shot event决定最终返回。等待期间
status flag、relation authority或pair liveness可能改变时，operation在consume/commit前重新取得所需owner fact。

buffer/backpressure的实际placement可由实现选择，但同一方向的read/write/poll必须使用同一个durable capacity/data
predicate。单个source token扩展、partial user progress与hangup precedence不得因consumer不同而分裂。

**违反表现：** read与poll使用不同empty条件、missed wake、HUP只存在event bit不在pair state、epoll route缓存live、
nonblock flag在file-private stale，或master/slave各复制Terminal queue truth。

**证明面：** current iomux/epoll contract audit；register-before/after transition；cancel/failure path；concurrent
producer/consumer/hangup；blocking/nonblocking and partial-progress matrix。

## DEVPTS-VFS-001 — Backend binding 与 pathname projection 保持两个 owner

devpts backend唯一拥有index、live`N -> pair`binding、initial metadata policy/source与logical retirement；VFS唯一
拥有resident inode metadata、dentry、cache/materialization与mounted pathname visibility。devpts只向VFS提供initial
metadata、lookup/readdir/publication/retire facts；pair只向devpts提交窄、幂等retire request。任何一方都不得读取
另一方private cache/container来推断truth。

master retirement后，devpts logical binding立即不再为新的backend lookup/open提供live pair。current VFS
dynamic-positive revocation gap允许已缓存旧dentry继续`stat`到inert inode，也可能让迟到materialization/cached
positive暂时遮蔽复用编号的新binding；该通用pathname availability/freshness缺口继续由VFS register拥有，不是PTY
implementation/cutover Stage。每次slave inode/open capability必须核验其捕获的pair identity/liveness并fail closed，绝不
能仅凭numeric `N`转接到当前new pair；没有旧cached projection的backend lookup仍按current binding取得new pair。

physical dentry/inode/storage reclaim可以晚于logical retirement。迟到materialization、cached hit、multiple mount view
和readdir cursor都不能反向恢复backend binding，也不能让devpts建立owner-local dentry generation作为替代VFS
protocol。

**违反表现：** devpts读取dentry map决定live、VFS inode refcount决定pair retire、旧inode/open capability仅凭复用的
numeric `N`命中新pair、retire等待physical reclaim才fail close，或pseudo filesystem特判绕过generic VFS owner。

**证明面：** backend fresh lookup、retire/open/reuse race、旧inode/open fail-close、current binding取得new pair、safe
index reuse与capacity内repeated churn。cached-positive、late materialization、multiple views与完整pathname
linearizability对current VFS open issue保持诚实的Not Proven边界，不升级为PTY acceptance requirement。

## PTY-REL-001 — 只有 slave semantic endpoint 参与 relation

PTY slave以stable terminal identity加入existing relation registry；master不加入relation，也不能通过opener、global PGID
或pair-local字段取得controlling authority。relation仍唯一拥有session binding、foreground selector和generation；task
topology、Signal与ThreadGroup job control继续拥有各自truth。

pathname与`TIOCGPTPEER`两条slave-open route都携带operation-local `O_NOCTTY`、read access与current caller
capability。全部fallible route/admission/opened-description prepare成功后，只有在未设置`O_NOCTTY`、description
可读、caller为session leader、caller session尚无controlling terminal且slave endpoint未绑定其它session时，relation
owner才原子建立binding并把caller current process group设为foreground。任一条件不成立都不改变relation；不得让
本来成功的slave open返回implicit-acquire errno，也不得重置已有relation或foreground selector。

relation commit只能进入必然成功的open publication tail；failed slave open不得留下短暂或持久relation。具体type/API
可以由实现选择，但relation commit后不得再有能把本次open变为失败的步骤。`O_NOCTTY`不进入description status truth；
PTY master始终不参与implicit acquisition，explicit `TIOCSCTTY`继续使用现有errno与relation protocol。

implicit acquisition、explicit `TIOCSCTTY`、foreground ioctl、background access和terminal control-character effect
都必须进入existing relation/task/signal handoff，并在owner guard外revalidate stable identity。

master retirement先让pair不再live/admissible，再请求relation owner按generation撤销对应relation；relation effect不能
由pair直接修改Session/ProcessGroup/Signal/job-control state。relation owner以旧generation与stable session-leader
identity形成snapshot，先撤销旧relation可发现性，再在guard外依次向旧controlling session leader thread group提交
`SIGHUP`、`SIGCONT`；没有live relation时不fallback，master-close也不向整个旧foreground group额外广播`SIGHUP`。
target外的session-exit、`TIOCNOTTY`、orphaned-pgrp与`TOSTOP` effects不因共享relation helper而被隐式启用。

**违反表现：** master成为controlling terminal、`O_NOCTTY`或write-only open仍implicit acquire、ineligible caller让
slave open失败、`TIOCGPTPEER`绕过共同policy、failed open遗留relation、pair缓存foreground PGID、master close直接写
ThreadGroup state、relation撤销失败后恢复pair live、stale generation signal新session，或PTY专用job-control旁路。

**证明面：** pathname/`TIOCGPTPEER`两条route的implicit acquisition、explicit `TIOCSCTTY`、`O_NOCTTY`、
read-vs-write-only、session-leader/non-leader、session/endpoint already-bound、failed-open-no-relation、wrong-session/
identity reuse、foreground/background access、terminal signals、master hangup relation teardown与target-excluded
disassociation effects；old-generation/stale-session suppression与`SIGHUP`-before-`SIGCONT` reference behavior。

## PTY-ABI-001 — 首版包络内不允许 success stub

本 RFC 接受的ioctl、flag、metadata、permission、stream与hangup surface必须真实执行并可由userspace观察。未知或
explicitly unsupported feature返回稳定、诚实errno；只有用户观察结果确实不变的flag才能silent compatibility，并按
仓库规则保留ABI取舍注释与诊断。slave initial metadata必须在allocation episode内真实形成，kernel不建立
额外grant mutation或历史`pt_chown` helper；不得用错误的fixed-root metadata、fake ioctl readback、虚构的
grant transition、shell prompt或test-specific branch替代目标能力。用户态PTY helper的版本、调用链和返回值不是
本R1 target或acceptance claim。

quota/index exhaustion返回`ENOSPC`，ordinary backing allocation failure返回`ENOMEM`；locked/permission/retired/invalid
flag与hangup errno遵循accepted Linux-default rule和implementation validation matrix。任何partial progress先返回已提交
用户单位，未提交suffix保持可重试；copy fault不得通过重复queue或回滚已发布cross-owner effect伪造原子性。

仓库内普通PTY Rust test app是长期guest-local acceptance consumer；它使用`anemone-rs`，形状与`socket-test`相同，
不调用libc PTY wrapper。其source/config/hash与Not Run写入执行证据，不硬编码为ABI判定条件。

**违反表现：** unsupported ioctl返回0、`O_NOCTTY`继续no-op却声称implicit acquisition兼容、quota映射`ENOMEM`、
用allocation smoke替代mandatory PTY test app覆盖、让userspace helper或libc调用链反向定义kernel target、把tmux/sshd
环境失败误写成PTY closure，或从单架构证据外推另一架构。

**证明面：** Linux 6.6.32 source/reference matrix、普通Rust + `anemone-rs` PTY test app、LTP、advisory tmux attempt、
optional sshd diagnostics、RV64/LA64 claim-separated evidence与negative/bypass audit。
