# ANE-CHG-20260724-alpha-omega-integration

**Type:** Integration
**Status:** Completed
**Date:** 2026-07-24
**Authors:** doruche, Codex
**Area:** build system / boot protocol / console / TTY / serial / documentation

## Problem

`dev/drc/alpha`已完成System Target Model R3，`dev/drc/omega`已完成TTY Subsystem R1。两条分支从
`05e73ef9`后独立推进，Git文本冲突只覆盖少数聚合文件，但存在跨文件语义冲突：alpha把initial-program
source、materialization与exec收口到`boot.rs`，omega把console selection、TTY endpoint publication与三份
real Terminal boot File收口为一次`InitStdio` handoff；omega的TTY Kconfig与acceptance wrapper仍使用合流前的
implicit build selection和固定rootfs路径。

## Scope

本轮只把两个已经关闭的RFC结果合流到一个共同基线：

- build orchestration、SystemTarget、Platform、BuildPreset和rootfs schema以alpha为真相源；
- TTY、console、NS16550A、TTY ABI、terminal job-control与TTY userspace acceptance以omega为真相源；
- 手工组合boot/TTY stdio handoff、Kconfig参数materialization与TTY acceptance wrapper；
- 合并双方current contract、register、RFC、transaction、navigation和双周devlog事实；
- 合流提交完成后，把现有omega worktree的独立分支fast-forward到同一共同基线。

本轮不改变typed initial-program source、TTY ABI、terminal/job-control target、Signal/topology owner、现有accepted
limitations或两个已关闭RFC的历史transaction，也不物理删除或重建omega私有工作区。

## Solution

alpha作为唯一merge receiver，以保留双亲历史的no-ff merge吸收omega。`main.rs`只拥有boot ordering：完成
console selection、Late init与console -> TTY publication后取得`InitStdio`，mount rootfs并将capability传给
`boot::exec_initial_program()`。`boot.rs`继续唯一解析`RootfsEntry | EmbeddedApp`、完成VFS materialization与
ordinary exec，同时按Read/Write/Write安装三份已经准备好的shared-Terminal File；它不重新打开anonymous console。

Kconfig保留alpha的resolved `KernelConfig`与`materialize_defaults()`边界，只把omega的TTY/UART容量字段、生成常量、
合法性校验和定向测试移入该形状。TTY wrapper使用显式`qemu-virt-rv64-pretest-release` preset和tracked
`kernel-image/disk-x0/disk-x1` bind，把测试盘master复制到worktree-local `build/runtime/`，不读取或切换root
`kconfig`，也不再复制acceptance rootfs到另一个manifest的输出路径。

`BOOT-PROTOCOL-001`只澄清已生效contract之间的局部义务：console拥有selection，TTY拥有Terminal File准备，boot
coordinator拥有publication顺序和`InitStdio`移交，Boot Protocol拥有stdio安装、root/cwd与ordinary exec。该澄清
不建立第二份selection或endpoint truth，也不改变用户可见TTY能力。

## Change

- 以`dev/drc/alpha@6530e4fd`为merge receiver，对`dev/drc/omega@e6079343`执行保留双亲的
  no-ff merge；共同base为`05e73ef9`。文本冲突按owner分域合并，TTY RFC、contract、transaction、register与
  navigation历史完整保留。
- boot顺序收口为`finish_boot_selection -> Late init -> boot_io::finalize -> mount_rootfs ->
  exec_initial_program`。`boot.rs`保留alpha的typed initial-program resolution，并消费omega准备的三份real
  Terminal File；production路径不再重新打开anonymous console。
- `KernelConfig::materialize_defaults()`继续是完整参数值的唯一边界，omega新增的TTY/UART容量、budget和poll参数
  均在该边界materialize，再由resolved config生成kernel definitions。
- TTY acceptance rootfs改用显式`fs.type = "folder"`和统一自动容量；wrapper改用
  `qemu-virt-rv64-pretest-release`、显式`kernel-image/disk-x0/disk-x1` binds与worktree-local runtime disk，
  删除旧`conf switch`、`--platform/--image`和fixed-rootfs copy bridge。
- RV64编译额外发现一处Git未标记的语义冲突：若干child modules经`prelude`的`crate::*`依赖crate-root
  可见的`kernel_execve`、`FdFlags`与`FsState`；boot实现移出`main.rs`后这些导入看似不再被root直接使用，
  冲突消解时曾被误删。现已在`anemone-kernel/src/main.rs`恢复该窄导入；没有增加第二boot owner或新接口。
- 双周devlog与小迭代索引增加本次合流入口，TTY RFC按其Closed lifecycle移入关闭导航。merge commit形成后，
  现有omega worktree以`--ff-only`接收共同基线并单独运行`just defconfig`；既有build日志和通道私有文件原地保留。

## Validation

以下均针对合流candidate重新执行，不把父分支历史结果当作本轮通过证据：

- 静态与host侧：`bash -n scripts/run-tty-test-rv64.sh`、`just xtask-test`（66/66）、
  `just fmt all --check`、`mdbook build docs`与`git diff --check`通过；source audit没有发现production
  anonymous-console boot route，`open_console_stdin()`的唯一剩余call site只在KUnit placeholder中出现。
- app/kernel build：`tty-test`与`jobctl-test`的RV64 app build通过；
  `qemu-virt-rv64-pretest-release`和`qemu-virt-la64-pretest-release`均完成kernel compile/link。
- RV64 TTY acceptance：`build/alpha-omega-integration-tty-rv64.log`记录239项KUnit通过、
  `TTYTEST:SUMMARY:PASS:45`、BusyBox vi/ash、binary output、ONLCR、drain ordering与host byte oracle通过，
  QEMU正常退出；BusyBox SHA-256为
  `fd9cb9dc66ba740dc94b055b564de0597453adfceef9be158b3774ca58b95241`。
- RV64 pretest：`build/alpha-omega-integration-user-test-rv64.log`以exit code 0结束，239项KUnit与全部
  `jobctl-test` case通过。signal + wait LTP共attempted 120、passed 106、failed 10、infra failed 0、
  skipped 4；10项失败均来自已知signal剩余限制，两个wait group为46/46。runner逐case打印的
  `FAIL LTP CASE ... : 0`不能替代case/group summary，不按字面误报为失败。
- 两份RV64日志均未出现kernel panic、deadlock、assertion failure或`BUG:`，结束后没有残留QEMU进程。
  按用户确认，本地环境不运行LA64 runtime；LA64 runtime、physical hardware与signal/wait之外的更广LTP
  均为**Not Run**。LA64 compile/link通过不外推为runtime或hardware证据。

## Tracking Issues

### CHG-001 - boot stdio owner形成并列真相源

**Status:** Neutralized
**Severity:** Keter

**Issue:** 若Boot Protocol在收到`InitStdio`后仍重新打开anonymous console，或TTY从console之外重算selection，
合流会同时保留两套boot stdio路线并违反`TTY-ENDPOINT-001`。

**Resolution:** source audit确认production boot只消费`boot_io::finalize()`发布的`InitStdio`；
`boot::exec_initial_program()`仅安装这三份File，不重新打开console。唯一剩余`open_console_stdin()`调用位于
KUnit placeholder。RV64 TTY acceptance与pretest均从同一路径正常进入userspace，未观察到第二stdio route。

### CHG-002 - TTY acceptance保留implicit build selection

**Status:** Neutralized
**Severity:** Keter

**Issue:** 原wrapper解析/切换root `kconfig`并使用旧`--platform/--image`入口，会绕过alpha已经生效的explicit-input
build contract。

**Resolution:** wrapper已改为显式release preset和三个declared binds，不读取或切换root `kconfig`，也不再使用
旧CLI或rootfs copy bridge；shell syntax、source audit和RV64 45/45 auto matrix均通过。

## Risk / Follow-up

- 合流candidate已重跑RV64 TTY自动matrix与signal/wait pretest；现有10项signal LTP失败仍按register限制处理，
  不是本次TTY/target-model合流新建的accepted gap。
- LA64只获得compile/link证据；runtime与hardware继续Not Run。未来若修改架构相关boot、serial或TTY路径，仍需
  独立LA64验证，不能以本次架构无关diff代替。
- omega `build/`中的Stage 4日志已在fast-forward前记录哈希并原地保留；本轮不执行会删除整个`build/`的
  `just clean`。

## Links

- Biweekly devlog: [2026-07-20 至 2026-08-02](../2026-07-20_to_2026-08-02.md)
- Current contracts: [`BOOT-PROTOCOL-001`](../../contracts/task/boot-protocol.md),
  [`TTY-ENDPOINT-001`](../../contracts/tty/data-plane.md#tty-endpoint-001--endpoint-publication是稳定的单向transaction)
- RFCs: [System Target Model](../../rfcs/system-target-model/index.md),
  [TTY Subsystem](../../rfcs/tty-subsystem/index.md)
- Historical transactions: [System Target Model](../transactions/2026-07-22-system-target-model.md),
  [TTY Subsystem](../transactions/2026-07-23-tty-subsystem.md)
