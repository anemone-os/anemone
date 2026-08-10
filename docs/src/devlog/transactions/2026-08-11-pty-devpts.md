# 2026-08-11 - PTY / devpts

**Status:** Active / R1 / Stage 1 Checkpoint 1 Closed
**Owners:** doruche, Codex
**Canonical Target:** [RFC-20260810-pty-devpts R1](../../rfcs/pty-devpts/index.md)
**Implementation Route:** [Stage 1--4](../../rfcs/pty-devpts/implementation.md)
**Contract Delta:** None；`PTY-DEVPTS-CUTOVER`与父RFC列出的全部Introduce/Refine ID仍Not Effective

## Scope

本transaction为长期、多Stage RFC保存checkpoint execution、review、validation与下一授权handoff。target、owner、
ABI、Contract Impact、acceptance与Stage路线仍只由canonical RFC及implementation拥有；本页不建立第二份计划或
current contract。开发者本轮只授权完成Stage 1 Checkpoint 1，不授权Checkpoint 2、Stage 2--4或任何contract cutover。

## Checkpoint Log

### 2026-08-11 - Stage 1 Checkpoint 1 activation and closure

**Change:** 从`dev/drc/alpha@32b6392f`的clean worktree激活Checkpoint 1。`TtyEndpoint`删除physical `TtyPort`，只保留
shared `Terminal`与不拥有状态的weak progress edge；physical attachment与worker持有`TtyPort`并在port-owned raw
RX/TX/idle predicate和Terminal之间搬运。boot-applied line snapshot由NS16550A driver作为owner-neutral值显式提交，
不再通过semantic endpoint或`TtyPort` trait泄漏。FileOps/relation继续只消费exact semantic endpoint与窄strong wake
capability；boot identity到endpoint的并行snapshot用常开assertion保持一一对应。旧helper、双路径、temporary adapter、
test-only facade、PTY/devpts surface与runtime relation lifecycle均未保留或引入。

inline owner-local KUnit新增/加强worker-spawn failure reservation rollback、pre-publication abort的
registry-before-stop/join与reference-cycle absence、notification只触发predicate recheck，以及selected physical identity
仍取得同一个semantic Terminal；既有duplicate attach、RX/TX partial progress与serial data-plane tests继续走production
transition。

**Review / Feedback:** 独立subagent首轮review为0 Apollyon / 0 Keter，指出一个Euclid：最初selected-endpoint test只按
硬编码index取数组元素，未经过physical identity到semantic endpoint的真实映射。实现删除该弱oracle，改为production
`select_endpoint()`同时消费parallel identity/endpoint snapshot、常开断言长度一致并按selected `TtyPortId`选择；KUnit
以`/soc/serial@2000`验证对应exact Terminal。复核确认该Euclid关闭，最终为0 Apollyon / 0 Keter / 0 Euclid。

source/bypass audit确认：semantic endpoint没有port/devnum/liveness/readiness truth；port只由
attachment/worker/driver持有；FileOps/relation没有physical representation bypass；wake count不参与行为；spawn failure
与abort cleanup exact且guards-out；没有public API/shared contract扩张、无第二状态真相、无临时双路径，也没有进入
Checkpoint 2或PTY-specific分支。

**Contract Cutover:** None。current TTY、opened-description、VFS、task/Signal/job-control contracts和register均未修改；
Stage 1整体与全部PTY能力尚未完成。

**Validation:** `git diff --check`与`just fmt kernel --check`通过。canonical
`./scripts/run-tty-test-rv64.sh --rootfs-sudo --busybox
etc/mounts/competition/preliminary/rootfs-rv/musl/busybox --sdcard etc/preliminary/images/sdcard-rv.img --mode auto
--log build/pty-devpts-stage1-ckpt1-rv64.log`在review修正后的最终candidate通过：repository RV64 release build完成，
618/618 KUnit通过；新增spawn-failure、abort/reference-cycle、selected identity/Terminal与既有TTY tests均为`ok`；guest
`TTYTEST:SUMMARY:PASS:50`，host报告`TTY-HARNESS:PASS:auto-byte-checks`与最终PASS，覆盖serial data plane、boot shared
Terminal、`/dev/tty` relation、BusyBox vi/ash并正常关机。首次不带`--rootfs-sudo`的同命令在进入kernel/QEMU前因
libguestfs/supermin不能读取host `/boot`停止；按LOCAL授权原样切换wrapper privilege mode后通过，不作为kernel失败。

LA64 build/runtime、PTY test app、LTP、tmux与sshd均Not Run，且不得从本次RV64证据外推。

**Next / Stop:** Checkpoint 1 Closed。执行严格停止在本checkpoint；Stage 1 Checkpoint 2仍Awaiting Authorization，
Stage 2--4保持Future，`PTY-DEVPTS-CUTOVER`仍Not Effective。下一步只能在维护者新的明确授权下进入Checkpoint 2。

## Current Handoff

当前live source已经让全部existing serial production caller使用semantic endpoint / physical attachment分离后的单一路径，
并保持serial ABI、devnum、boot fd、console owner、Terminal和relation行为。transaction保持Active只因为父RFC的后续
checkpoints/stages尚未执行；本记录不授权自动继续，也不把Checkpoint 1证据写成Stage 1或PTY target closure。
