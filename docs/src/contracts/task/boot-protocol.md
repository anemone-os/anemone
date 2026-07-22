# Anemone Boot Protocol 当前契约

**Contract ID：** `BOOT-PROTOCOL`
**状态：** Active
**Owner：** initial user-program boot protocol
**参与领域：** rootfs materialization / kernel boot / VFS / exec-binfmt / user entry
**覆盖范围：** 当前 rootfs metadata 选择初始用户程序、kernel 侧准备与 ordinary exec handoff
**不覆盖：** system target、embedded app、Linux init discovery、PID 1 supervision或root device选择
**实现位置：** `scripts/xtask/src/{config/rootfs.rs,tasks/rootfs/mkfs.rs}`、`anemone-kernel/src/main.rs`
**依赖：** `USER-ENTRY-002` 与现有 VFS / exec-binfmt contract
**Pending Successor：** [RFC-20260722-system-target-model R0](../../rfcs/system-target-model/index.md) 已接受；其 Refine target 尚未 cut over，当前规则继续生效
**最后核验：** 2026-07-23

## BOOT-PROTOCOL-001 — rootfs metadata选择初始用户程序

**规则：** rootfs manifest 的 `[init].path` 是当前 initial-program selection owner。Rootfs
materializer把该字符串原样写入 `/.anemone/init`；producer必须提供绝对 VFS path，当前格式不
包含 argv、envp、fallback list或typed source tag。Kernel完成root mount与late init后读取完整
metadata文本，准备初始console stdin/stdout/stderr以及root/cwd `FsState`，再以该path同时作为
executable path和`argv[0]`调用ordinary `kernel_execve()`。VFS path resolution、ELF/shebang
dispatch、credential/exec处理与mandatory user-entry gate继续由既有owner负责。

Metadata读取失败、path解析或exec失败都终止当前boot；kernel不搜索`/sbin/init`等fallback，
不解析rootfs manifest，也不按workload name选择分支。当前kernel固定提供既有初始envp；本次
baseline提取只记录effective行为，不把该envp提升为跨RFC可扩展配置面。

**唯一owner与局部义务：** rootfs manifest/materializer拥有metadata值与原样publication；kernel
Boot Protocol拥有读取、初始stdio/root/cwd准备和ordinary exec handoff；VFS / exec-binfmt / user-entry
各自拥有handoff后的路径、格式与用户态进入语义。任一参与方不得缓存另一份可变initial-program
selection或绕过ordinary exec建立第二条loader。

**违反表现：** platform、Kconfig或kernel workload branch另存一份init selection；rootfs producer
写入一条path而kernel执行另一条；metadata读取失败后静默启动其它程序；initial program绕过VFS、
ELF/shebang或mandatory user-entry gate。

**验证 / Enforcement：** `RootfsCtx::gen_init_config()`、`mount_rootfs()`、`exec_init_proc()`与
`kernel_execve()` source closure；tracked rootfs manifests的绝对`[init].path`审计；受支持rootfs
boot smoke验证configured init实际启动。当前contract提取未新增runtime验证或改变既有代码。

**最初来源：** 现有rootfs materializer与kernel `exec_init_proc()`实现。

**当前来源：** 2026-07-22 current-contract extraction；[RFC-20260722-system-target-model promotion preflight](../../rfcs/system-target-model/implementation.md#0a-promotion-preflight-结论2026-07-22)。
