# Anemone Boot Protocol 当前契约

**Contract ID：** `BOOT-PROTOCOL`
**状态：** Active
**Owner：** initial user-program boot protocol
**参与领域：** rootfs materialization / kernel boot / console / TTY / VFS / exec-binfmt / user entry
**覆盖范围：** SystemTarget选择typed initial-program source、build生成有限boot input、kernel侧
VFS materialization与ordinary exec handoff
**不覆盖：** Linux init discovery、PID 1 supervision、root device选择或通用artifact/runtime registry
**实现位置：** `scripts/xtask/src/{config/system_target.rs,tasks/app/build.rs,tasks/build/mod.rs}`、
`anemone-kernel/src/{boot.rs,main.rs,device/boot_io.rs}`
**依赖：** `USER-ENTRY-002`、[`TTY-ENDPOINT-001`](../tty/data-plane.md#tty-endpoint-001--endpoint-publication是稳定的单向transaction)
与现有 VFS / exec-binfmt contract
**Pending Successor：** None
**最后核验：** 2026-07-24

## BOOT-PROTOCOL-001 — typed initial-program source统一收口到普通 VFS exec

**规则：** SystemTarget是initial-program selection owner，只接受closed、typed的
`RootfsEntry | EmbeddedApp { app }`，且两种variant都可携带optional完整非空argv。Kernel build把选择解析为
generated Rust boot input；kernel不解析SystemTarget或app manifest。两种source最终都提供稳定绝对VFS path；
显式argv包含`argv[0]`并逐项原样传递，kernel不插入path或要求两者相等，省略argv时才以resolved path作为
唯一参数。两种路径都保持既有五项env、由boot I/O composition移交的real Terminal stdin/stdout/stderr与
root/cwd `FsState`，并统一调用ordinary
`kernel_execve()`。VFS path resolution、ELF/shebang dispatch、credential/exec处理与mandatory user-entry
gate继续由既有owner负责。

`RootfsEntry`生成typed tag和optional argv；kernel在root mount与late init后读取rootfs materializer发布的
`/.anemone/init`完整metadata文本。该metadata保持path-only，不承载argv或复合格式。`EmbeddedApp`通过唯一
公共`build_app()` exporter解析target引用，要求
manifest name与reference一致，并在kernel compile前拒绝非单一、非普通或没有execute bit的export。
Generated input以`include_bytes!`直接引用本轮export。Runtime在缺失时创建`0755`的`/.anemone`，随后为
本boot overmount独有ramfs；以独占`0600` temp完整写入bytes，改为`0555`后用no-replace rename发布为
`/.anemone/embedded-init`。Ramfs保持挂载，使ordinary exec/binfmt和shebang interpreter能重新打开同一
稳定path；持久rootfs不拥有published executable truth。

Build、metadata读取、materialization、path解析或exec任一步失败都终止当前boot；kernel不搜索fallback，
不按workload name选择分支，也不让`RootfsEntry`遮盖`EmbeddedApp`失败。Embedded runtime failure不主动
rollback已经boot-fatal的ramfs/temp；后续boot挂载新的ramfs，不能把前一boot未完成或已发布的文件误认成
本轮initial program，持久rootfs最多留下空`/.anemone`目录。当前固定envp不是跨RFC可扩展配置面。

**唯一owner与局部义务：** SystemTarget拥有source与app reference；build resolver/materializer拥有
reference、export与有限generated input；rootfs manifest/materializer只拥有`RootfsEntry` metadata值与原样
publication；console拥有boot selection，TTY按该immutable selection重验并准备三份real Terminal File，boot
coordinator按console -> TTY顺序完成单向publication并移交窄`InitStdio` capability；kernel Boot Protocol拥有
runtime source resolution、EmbeddedApp publication、初始stdio安装、root/cwd准备和ordinary exec handoff；
VFS / exec-binfmt / user-entry各自拥有handoff后的路径、格式与用户态进入语义。
任一参与方不得缓存另一份可变selection、建立第二materializer/loader或把anonymous bytes直接交给exec。

**违反表现：** Platform、Kconfig或kernel workload branch另存一份initial-program selection；kernel解析
target/app manifest；build接受ambiguous/non-executable export；EmbeddedApp只保留首次probe bytes而使shebang
reopen失败；publication失败后进入fallback；boot绕过`InitStdio`重新打开anonymous console files；initial program
绕过VFS、ELF/shebang或mandatory user-entry gate。

**验证 / Enforcement：** SystemTarget/parser/generator、公共app exporter、generated-input/clean与kernel
Boot Protocol source closure；55项xtask tests、全部tracked Platform/SystemTarget schema validation与
RV64/LA64 release build；此前production RootfsEntry RV64 build/boot smoke、现有init作为
Embedded ELF、同一路径shebang reopen和missing-interpreter boot-fatal的RV64 QEMU smoke；不clean修改Source
artifact后kernel bytes/hash变化；latest-byte independent review为Apollyon 0 / Keter 0。

**最初来源：** 现有rootfs materializer与kernel `exec_init_proc()`实现。

**当前来源：** [RFC-20260722-system-target-model R6](../../rfcs/system-target-model/index.md)；
[Checkpoint R6A cutover transaction](../../devlog/transactions/2026-07-24-system-target-model-r6-bind-argv.md)；
[Checkpoint 5A baseline transaction](../../devlog/transactions/2026-07-22-system-target-model.md#checkpoint-5a-closure-and-boot-protocol-cutover---2026-07-24)；
[alpha/omega RFC结果合流小迭代](../../devlog/changes/2026-07-24-alpha-omega-integration.md)。
