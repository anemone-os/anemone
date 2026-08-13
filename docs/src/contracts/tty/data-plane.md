# TTY Data Plane 当前契约

**Contract ID：** `TTY-PORT-001` / `TTY-TERM-001` / `TTY-INPUT-001` / `TTY-OUTPUT-001` / `TTY-ENDPOINT-001`
**状态：** Active
**Owner：** `device::tty` data-plane protocol；UART physical state 仍由 serial driver 唯一拥有
**参与领域：** serial driver / console / TTY / devfs / VFS / boot stdio
**覆盖范围：** boot-applied serial capability、ordered RX condition handoff、serial与PTY共享 Terminal truth、canonical/raw input、termios input conditioning、byte output、readiness、termios/winsize data-plane 与稳定 serial endpoint publication
**不覆盖：** controlling-terminal relation、caller-relative `/dev/tty`、foreground/background access、terminal-generated signal、relation cleanup、physical runtime line reconfiguration或hotplug；PTY pair/hangup由companion contract定义
**实现位置：** `anemone-kernel/src/device/tty/`、`anemone-kernel/src/driver/serial/ns16550a/`、`anemone-kernel/src/device/{boot_io,console,devnum}.rs`、`anemone-kernel/src/main.rs`
**依赖：** None；本页定义后续 TTY relation/job-control contract 使用的数据面 baseline
**Companion Contract：** [TTY controlling relation 与 job control](./job-control.md) 中的 `TTY-REL-001`、`TTY-JOBCTL-001`、`TTY-LIFE-001` 与 `TTY-ABI-001`（Active）
**最后核验：** 2026-08-13

## 状态与能力所有权

| 状态 / 能力 | 唯一 Owner | 其它参与方持有什么 | 行为用途 |
| --- | --- | --- | --- |
| MMIO、IRQ、FIFO、boot-applied line、ordered raw RX unit handoff、break/fault/overrun/overflow counter与最终TX serialization | UART physical driver | TTY持窄`TtyPort` capability；console持output-only projection | bounded RX publication与console/TTY TX |
| committed termios、winsize、discipline、editable/committed input、output queue、逻辑输出列与readiness predicate | endpoint共享的`Terminal` | opened file持Terminal引用；operation ctx提供live flags | read/write/poll/ioctl data plane |
| worker-local dequeued batch | 单次deferred-consumer invocation | notification只要求predicate重验 | raw handoff到discipline的短命ownership transfer |
| immutable port identity到`ttyS<N>`映射及published endpoint | TTY endpoint registry | devfs持open provider；boot只消费selected identity | stable node、device number与shared Terminal lookup |
| selected-console truth与`/dev/console` | console owner | TTY只在boot finalize重验selected-terminal identity | 安装real Terminal boot fd，不转移console owner |

Event/wake edge、diagnostic owner/name/counter与测试 marker 都不是行为真相源；它们不得反向驱动
input、readiness、publication或TX progress。

## TTY-PORT-001 — 物理端口与 raw handoff 只有一个 owner

**规则：** UART driver唯一拥有MMIO、IRQ、hardware FIFO、boot-applied line configuration、line-error observation、
IRQ到deferred consumer之间的bounded raw RX handoff，以及console/TTY共享UART的最终TX serialization。
TTY只能持窄`TtyPort` capability，不复制register、FIFO、applied-line或raw-queue truth。

NS16550A固定同时提供output-only console projection与`TtyPort`，不实现或注册raw serial `CharDev`。console不消费
RX、不提交runtime line configuration；pre-publish consumer binding是RX唯一去向。hard IRQ只执行bounded drain、
hardware-status classification、raw publication、counter与窄notification，不sleep、不做user copy/line discipline/
signal/user-wait wake或递归UART日志。每个sample规范化为ordered `Byte`、`Break`或`FaultedByte`；break优先于同一
sample的parity/framing classification，overrun只计数而不制造虚构byte。LSR condition与payload不得拆成sideband
或第二队列，read-clear LSR/RBR只能在消费对应sample时读取。raw overflow保持旧units并可审计；所有普通console/
TTY TX经过同一个port-owned IRQ-safe serialization，任意用户write不能形成无界IRQ-off临界区。

**违反表现：** UART与TTY各保存一份raw queue、byte/status sideband失配、console与TTY双RX consumer、raw major
234重新出现、TTY访问register、batch边界破坏下一sample的read-clear status、overflow静默覆盖旧unit、IRQ执行
sleepable/递归效果，或console/TTY绕过同一TX owner。

**验证 / Enforcement：** fixed raw-unit ring与port capability source/lock audit；status decoder、break priority、
fault payload、no-payload break、overrun、whole-unit overflow、bounded IRQ drain、partial TX与raw FIFO KUnit；RV64
repository build/QEMU自动matrix；raw 234、sideband condition、console RX、direct-register和polling-watchdog bypass audit。

**最初来源：** [RFC-20260722-tty-subsystem R1](../../rfcs/tty-subsystem/index.md)。

**当前来源：** [`TTY-DATA-CUTOVER` transaction](../../devlog/transactions/2026-07-23-tty-subsystem.md#stage-2--checkpoint-4-closure与-tty-data-cutover---2026-07-23)；[Serial TTY RX conditioning 小迭代](../../devlog/changes/2026-08-03-tty-serial-rx-conditioning.md)。

## TTY-TERM-001 — Endpoint共享唯一terminal semantic truth

**规则：** 同一endpoint的所有attachment引用同一个`Terminal`。它唯一持有committed termios、winsize、
concrete discipline、canonical pending edit、committed/noncanonical input、output queue、逻辑output-processing列和
readiness predicate。committed termios内含endpoint-specific control profile：serial保存driver boot-applied physical
line的immutable projection；PTY保存不驱动物理线路的logical `c_cflag`。两者都和其它termios字段由同一个generation
transaction读取、重验并一次提交，不得在port、pair、FileOps或devpts中建立第二份control truth。逻辑列与完整output
token一起提交；它只描述TTY output processor已经接受的stream位置，
不是UART、console或host terminal的物理cursor truth。opened file只保存Terminal引用；`O_NONBLOCK`每次来自通用
open-file-description flags。`IGNBRK`、`BRKINT`、`IGNPAR`、`PARMRK`、
`INPCK`、`ISTRIP`、`INLCR`、`IGNCR`与`ICRNL`只改变Terminal的input interpretation，不反向修改UART line。
用户可见属性只有在完整candidate可提交时一次性发布；invalid、不可表示或要求未支持hardware action的update失败
并保持旧snapshot，不能success-stub。

**违反表现：** 不同fd观察冲突termios/winsize/input、file缓存stale nonblock、UART/console保存或反向驱动第二份
output列truth、失败update部分可见，或ioctl成功但丢弃用户状态。

**验证 / Enforcement：** shared fd0/1/2与新开`ttyS0`交叉termios/winsize matrix；九个input flags与组合round-trip、
`TAB0/TAB3` round-trip、unsupported rollback、`stty`、native Python 3.13 PyREPL、GNU `less 668` user run、
`TCSETSW/F`和owner/source audit；setter generation/revalidation assertion与KUnit。

**最初来源：** [RFC-20260722-tty-subsystem R1](../../rfcs/tty-subsystem/index.md)。

**当前来源：** [`TTY-DATA-CUTOVER` transaction](../../devlog/transactions/2026-07-23-tty-subsystem.md#stage-2--checkpoint-4-closure与-tty-data-cutover---2026-07-23)；[Serial TTY RX conditioning 小迭代](../../devlog/changes/2026-08-03-tty-serial-rx-conditioning.md)；[TTY TAB3/XTABS output processing 小迭代](../../devlog/changes/2026-08-08-tty-tab3-output.md)；[PTY logical cflag profile 小迭代](../../devlog/changes/2026-08-13-pty-logical-cflag.md)。

**PTY refine：** PTY slave与master attachment复用同一个Terminal；pair只拥有lifecycle/peer predicate，不复制termios、
winsize、discipline、stream或readiness truth。该refine来自[`PTY-DEVPTS-CUTOVER`](./pty-devpts.md)。

## TTY-INPUT-001 — Input ownership、record boundary与readiness同源

**规则：** raw dequeue是port到worker-local batch的ownership transfer；discipline提交后，editable/committed input与
readiness只归共享Terminal。canonical mode在delimiter、`VEOF`或明确flush前不发布半条record；`VERASE/VKILL`
只修改pending edit，read不跨越已提交record boundary。noncanonical `VMIN=1,VTIME=0`提供真实byte stream。
blocking read、poll/select和deferred consumer都使用durable predicate的publication + recheck，notification不携带
work truth。一次read只消费所选prefix；显式flush、已记录overflow与通用post-validation copy-fault边界之外，普通
路径不得重复、凭空产生或越界消费input。

每个ordered RX unit取得当次committed termios snapshot后由Terminal解释。normal byte先执行`ISTRIP`，再按
`IGNCR > ICRNL`处理CR、按`INLCR`处理其余NL；`PARMRK`下有效`0xff`以literal `0xff 0xff`提交。break按
`IGNBRK > BRKINT > PARMRK > NUL`处理；faulted byte在`INPCK`关闭时重入normal pipeline，开启时按
`IGNPAR > PARMRK > NUL`处理。break/fault marker与replacement NUL都是literal，不再触发strip、CR/NL、control
character、delimiter或echo。一个condition扩展的2/3-byte token必须先通过完整容量检查再一次提交；backpressure
时worker cursor保留同一unit重试，不得暴露prefix、重复marker或丢失后续unit。

**违反表现：** lost work/wake、半行使poll readable、read跨record、wake count成为input truth、concurrent drain
丢失可读状态，或为copy fault建立第二份rollback queue。

**验证 / Enforcement：** canonical newline/erase/kill/EOF/short-record、input-conditioning matrix、literal marker
atomic retry、raw VMIN1、nonblock EAGAIN和poll/pselect RV64 matrix；record/queue accounting、read/poll predicate、
register-plus-recheck与worker batch assertion/KUnit；人工`VERASE/VKILL/VEOF`边界复验。

**最初来源：** [RFC-20260722-tty-subsystem R1](../../rfcs/tty-subsystem/index.md)。

**当前来源：** [`TTY-DATA-CUTOVER` transaction](../../devlog/transactions/2026-07-23-tty-subsystem.md#stage-2--checkpoint-4-closure与-tty-data-cutover---2026-07-23)；[Serial TTY RX conditioning 小迭代](../../devlog/changes/2026-08-03-tty-serial-rx-conditioning.md)。

**PTY refine：** master write按同一input-conditioning/discipline规则直接提交ordered bytes；master hangup清除committed
slave input。readiness仍由Terminal input与pair peer predicate组合，pair不建立第二队列。该refine来自
[`PTY-DEVPTS-CUTOVER`](./pty-devpts.md)。

## TTY-OUTPUT-001 — 输出按用户byte计量并由port最终序列化

**规则：** TTY write接受任意bytes。`OPOST`关闭时原样提交；启用transform时，partial progress按已经消费的
用户输入bytes计量，单个input byte的扩展不能重复提交。`TAB0`保留literal tab，`TAB3/XTABS`按已经提交的逻辑列把
tab原子展开到下一个8列边界，形成1至8个空格；CR、backspace、newline/`ONLCR`与ordinary/control byte按同一
output-processing列规则推进。完整token进入Terminal queue后才同时推进源byte progress与逻辑列；backpressure不
提交token prefix，也不推进二者。output flush丢弃尚未提交port的backend work，但不倒退已经接受的stream位置。

echo复用同一Terminal transform与port capability，但不在Terminal guard内等待hardware。blocking write与poll使用
同一个Terminal-owned writable predicate，并为当前termios与逻辑列上任意一个完整source-byte token保留空间；
writable、drain、partial write与`TCSETSW`来自真实backend progress。panic/early-console best-effort路径不是普通TTY
TX truth。普通console record与整次TTY write不承诺相互原子，但都必须经同一个port owner按bounded batch序列化。

**违反表现：** binary byte因UTF-8失败、ONLCR/TAB3 partial progress重发输入、TAB3 backpressure改变后续列、
write wait与poll使用冲突的writable条件、echo持Terminal guard等待、虚构drain，或console/TTY形成两套TX truth。

**验证 / Enforcement：** binary NUL/`0xff`、OPOST/ONLCR、TCSETSW payload-before-marker、TCSETSF与drain RV64
byte oracle；TAB3列推进、完整token backpressure与readiness inline KUnit compile/source audit；GNU `less 668`
进入可用全屏界面并以`q`退出的用户运行证据；final output/summary drain后再关机。

**最初来源：** [RFC-20260722-tty-subsystem R1](../../rfcs/tty-subsystem/index.md)。

**当前来源：** [`TTY-DATA-CUTOVER` transaction](../../devlog/transactions/2026-07-23-tty-subsystem.md#stage-2--checkpoint-4-closure与-tty-data-cutover---2026-07-23)；[TTY TAB3/XTABS output processing 小迭代](../../devlog/changes/2026-08-08-tty-tab3-output.md)。

**PTY refine：** slave write与echo经过同一output processing进入Terminal queue并由master消费；partial progress、drain和
writability保持Terminal-owned，master不是physical port且不伪造`TtyPort`。该refine来自
[`PTY-DEVPTS-CUTOVER`](./pty-devpts.md)。

## TTY-ENDPOINT-001 — Endpoint publication是稳定的单向transaction

**规则：** 每个启动期成功注册的TTY-capable serial port从immutable identity获得确定性的逻辑实例号；编号不依赖
probe完成顺序。publish前完成identity唯一性校验、Terminal/raw handoff、deferred consumer与open provider的全部
fallible prepare；devfs publish是可见线性化点。成功后`/dev/ttyS<N>`名称、major 4/minor `64+N`、endpoint identity
和共享Terminal保持到重启；第一版不支持runtime unpublish、重新编号或复用。console owner独立发布major 5/minor 1
的`/dev/console`并持selected truth；boot fd0/1/2安装被选中endpoint的真实shared Terminal，但不由此取得controlling relation。

**违反表现：** node先于consumer可用、失败留下半发布endpoint、编号随probe顺序漂移、last close删除Terminal、TTY
接管`/dev/console`，或boot stdio仍使用anonymous EOF console file。

**验证 / Enforcement：** deterministic identity/duplicate/minor-overflow与prepare-before-publish KUnit；RV64 guest核对
`ttyS0` 4:64、`console` 5:1及boot三fd/shared reopen truth；全树anonymous boot caller、duplicate publisher、direct
registry/port bypass audit。

**最初来源：** [RFC-20260722-tty-subsystem R1](../../rfcs/tty-subsystem/index.md)。

**当前来源：** [`TTY-DATA-CUTOVER` transaction](../../devlog/transactions/2026-07-23-tty-subsystem.md#stage-2--checkpoint-4-closure与-tty-data-cutover---2026-07-23)。

## 跨领域局部义务

| Parent Contract / Obligation | 参与方 | 必须完成的动作 | Handoff / 线性化点 | 失败 / Cleanup责任 |
| --- | --- | --- | --- | --- |
| `TTY-PORT-001` / RX | UART / worker / discipline | UART发布bounded ordered units；worker只按predicate取走；discipline一次提交或保留当前cursor | raw dequeue -> worker batch -> Terminal atomic commit | whole-unit overflow由port计数；backpressure不推进cursor；未发布endpoint只回滚本地对象 |
| `TTY-OUTPUT-001` / TX | Terminal / UART / console | Terminal提交converted batch；UART唯一序列化实际progress | backend accept/progress | partial按用户byte诚实返回；guard外等待drain |
| `TTY-ENDPOINT-001` / publish | TTY registry / devfs / boot | 完成全部fallible prepare后单向发布，再安装已选Terminal boot files | devfs publication；boot finalize | publish前abort；publish后不unpublish/reuse |

## 当前接受边界

- 本页只定义serial TTY data plane；`/dev/tty`、controlling relation和terminal job control由已生效的
  [companion contract](./job-control.md)定义，不能从本页单独推断。
- build/runtime acceptance只在RV64验证；LA64 compile/runtime、实体UART parity/framing injection与hardware均Not Run，
  classifier/KUnit和RV64 QEMU break结果不得外推。
- runtime line reconfiguration、physical hardware hangup/backend fatal、hotplug/unpublish、完整`VMIN/VTIME`与完整Linux
  termios/ioctl corner不在本页；PTY pair、devpts与master hangup见[companion contract](./pty-devpts.md)。
- post-validation user-copy fault不提供TTY-local rollback/replay；普通有效buffer read、record boundary与未选后缀仍受
  `TTY-INPUT-001`约束。
