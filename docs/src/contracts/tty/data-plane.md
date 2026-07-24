# Serial TTY Data Plane 当前契约

**Contract ID：** `TTY-PORT-001` / `TTY-TERM-001` / `TTY-INPUT-001` / `TTY-OUTPUT-001` / `TTY-ENDPOINT-001`
**状态：** Active
**Owner：** `device::tty` data-plane protocol；UART physical state 仍由 serial driver 唯一拥有
**参与领域：** serial driver / console / TTY / devfs / VFS / boot stdio
**覆盖范围：** boot-applied serial capability、共享 Terminal truth、canonical/raw input、byte output、readiness、termios/winsize data-plane 与稳定 `/dev/ttyS<N>` publication
**不覆盖：** controlling-terminal relation、caller-relative `/dev/tty`、foreground/background access、terminal-generated signal、relation cleanup、runtime line reconfiguration、hangup、hotplug 或 PTY
**实现位置：** `anemone-kernel/src/device/tty/`、`anemone-kernel/src/driver/serial/ns16550a/`、`anemone-kernel/src/device/{boot_io,console,devnum}.rs`、`anemone-kernel/src/main.rs`
**依赖：** None；本页定义后续 TTY relation/job-control contract 使用的数据面 baseline
**Companion Contract：** [TTY controlling relation 与 job control](./job-control.md) 中的 `TTY-REL-001`、`TTY-JOBCTL-001`、`TTY-LIFE-001` 与 `TTY-ABI-001`（Active）
**最后核验：** 2026-07-24

## 状态与能力所有权

| 状态 / 能力 | 唯一 Owner | 其它参与方持有什么 | 行为用途 |
| --- | --- | --- | --- |
| MMIO、IRQ、FIFO、boot-applied line、raw RX handoff、line-error/overflow counter与最终TX serialization | UART physical driver | TTY持窄`TtyPort` capability；console持output-only projection | bounded RX publication与console/TTY TX |
| committed termios、winsize、discipline、editable/committed input与readiness predicate | endpoint共享的`Terminal` | opened file持Terminal引用；operation ctx提供live flags | read/write/poll/ioctl data plane |
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
raw publication、counter与窄notification，不sleep、不做user copy/line discipline/signal/user-wait wake或递归UART日志。
raw overflow保持旧数据并可审计；所有普通console/TTY TX经过同一个port-owned IRQ-safe serialization，任意用户
write不能形成无界IRQ-off临界区。

**违反表现：** UART与TTY各保存一份raw queue、console与TTY双RX consumer、raw major 234重新出现、TTY访问
register、overflow静默覆盖旧byte、IRQ执行sleepable/递归效果，或console/TTY绕过同一TX owner。

**验证 / Enforcement：** fixed raw ring与port capability source/lock audit；overflow、line error、bounded IRQ drain、
partial TX与raw FIFO KUnit；RV64 repository build/QEMU自动matrix；raw 234、console RX、direct-register和polling-watchdog
bypass audit。

**最初来源：** [RFC-20260722-tty-subsystem R1](../../rfcs/tty-subsystem/index.md)。

**当前来源：** [`TTY-DATA-CUTOVER` transaction](../../devlog/transactions/2026-07-23-tty-subsystem.md#stage-2--checkpoint-4-closure与-tty-data-cutover---2026-07-23)。

## TTY-TERM-001 — Endpoint共享唯一terminal semantic truth

**规则：** 同一serial endpoint的所有open file引用同一个`Terminal`。它唯一持有committed termios、winsize、
concrete discipline、canonical pending edit、committed/noncanonical input和readiness predicate。opened file只保存
Terminal引用；`O_NONBLOCK`每次来自通用open-file-description flags。用户可见属性只有在完整candidate可提交时
一次性发布；invalid、不可表示或要求未支持hardware action的update失败并保持旧snapshot，不能success-stub。

**违反表现：** 不同fd观察冲突termios/winsize/input、file缓存stale nonblock、port shadow反向驱动Terminal、
失败update部分可见，或ioctl成功但丢弃用户状态。

**验证 / Enforcement：** shared fd0/1/2与新开`ttyS0`交叉termios/winsize matrix；unsupported rollback、`stty`
round-trip、`TCSETSW/F`和owner/source audit；setter generation/revalidation assertion与KUnit。

**最初来源：** [RFC-20260722-tty-subsystem R1](../../rfcs/tty-subsystem/index.md)。

**当前来源：** [`TTY-DATA-CUTOVER` transaction](../../devlog/transactions/2026-07-23-tty-subsystem.md#stage-2--checkpoint-4-closure与-tty-data-cutover---2026-07-23)。

## TTY-INPUT-001 — Input ownership、record boundary与readiness同源

**规则：** raw dequeue是port到worker-local batch的ownership transfer；discipline提交后，editable/committed input与
readiness只归共享Terminal。canonical mode在delimiter、`VEOF`或明确flush前不发布半条record；`VERASE/VKILL`
只修改pending edit，read不跨越已提交record boundary。noncanonical `VMIN=1,VTIME=0`提供真实byte stream。
blocking read、poll/select和deferred consumer都使用durable predicate的publication + recheck，notification不携带
work truth。一次read只消费所选prefix；显式flush、已记录overflow与通用post-validation copy-fault边界之外，普通
路径不得重复、凭空产生或越界消费input。

**违反表现：** lost work/wake、半行使poll readable、read跨record、wake count成为input truth、concurrent drain
丢失可读状态，或为copy fault建立第二份rollback queue。

**验证 / Enforcement：** canonical newline/erase/kill/EOF/short-record、ICRNL、raw VMIN1、nonblock EAGAIN和
poll/pselect RV64 matrix；record/queue accounting、read/poll predicate、register-plus-recheck与worker batch assertion/KUnit；
人工`VERASE/VKILL/VEOF`边界复验。

**最初来源：** [RFC-20260722-tty-subsystem R1](../../rfcs/tty-subsystem/index.md)。

**当前来源：** [`TTY-DATA-CUTOVER` transaction](../../devlog/transactions/2026-07-23-tty-subsystem.md#stage-2--checkpoint-4-closure与-tty-data-cutover---2026-07-23)。

## TTY-OUTPUT-001 — 输出按用户byte计量并由port最终序列化

**规则：** TTY write接受任意bytes。`OPOST`关闭时原样提交；启用transform时，partial progress按已经消费的
用户输入bytes计量，单个input byte的扩展不能重复提交。echo复用同一Terminal transform与port capability，但不在
Terminal guard内等待hardware。writable、drain、partial write与`TCSETSW`来自真实backend progress；panic/early-console
best-effort路径不是普通TTY TX truth。普通console record与整次TTY write不承诺相互原子，但都必须经同一个port owner
按bounded batch序列化。

**违反表现：** binary byte因UTF-8失败、ONLCR partial progress重发输入、echo持Terminal guard等待、虚构drain，
或console/TTY形成两套TX truth。

**验证 / Enforcement：** binary NUL/`0xff`、OPOST/ONLCR、TCSETSW payload-before-marker、TCSETSF与drain RV64
byte oracle；partial backend progress和transform KUnit/source audit；final output/summary drain后再关机。

**最初来源：** [RFC-20260722-tty-subsystem R1](../../rfcs/tty-subsystem/index.md)。

**当前来源：** [`TTY-DATA-CUTOVER` transaction](../../devlog/transactions/2026-07-23-tty-subsystem.md#stage-2--checkpoint-4-closure与-tty-data-cutover---2026-07-23)。

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
| `TTY-PORT-001` / RX | UART / worker / discipline | UART发布bounded raw batch；worker只按predicate取走；discipline一次提交 | raw dequeue -> worker batch -> Terminal commit | overflow由port计数；未发布endpoint只回滚本地对象 |
| `TTY-OUTPUT-001` / TX | Terminal / UART / console | Terminal提交converted batch；UART唯一序列化实际progress | backend accept/progress | partial按用户byte诚实返回；guard外等待drain |
| `TTY-ENDPOINT-001` / publish | TTY registry / devfs / boot | 完成全部fallible prepare后单向发布，再安装已选Terminal boot files | devfs publication；boot finalize | publish前abort；publish后不unpublish/reuse |

## 当前接受边界

- 本页只定义serial TTY data plane；`/dev/tty`、controlling relation和terminal job control由已生效的
  [companion contract](./job-control.md)定义，不能从本页单独推断。
- build/runtime acceptance只在RV64验证；LA64 compile/runtime与hardware均Not Run，RV64结果不得外推。
- runtime line reconfiguration、hardware hangup/backend fatal、hotplug/unpublish、完整`VMIN/VTIME`、PTY/devpts/ptmx
  与完整Linux termios/ioctl corner不在本页。
- post-validation user-copy fault不提供TTY-local rollback/replay；普通有效buffer read、record boundary与未选后缀仍受
  `TTY-INPUT-001`约束。
