# ANE-CHG-20260811-kunit-execution-proof

**Type:** Cleanup / validation contract
**Status:** Completed
**Date:** 2026-08-11
**Authors:** doruche, Codex
**Area:** KUnit / scheduler and kthread test shape / validation proof / production test seams

## Problem / Context

KUnit的定位是boot-integrated、快速、串行的in-kernel unit test，但现有suite已经包含scheduler、wait、kthread、timer、
IPI及多个共享live-kernel fixture。2026-08-02小迭代定义了runner时点、current task、cleanup、panic与实际environment
证明边界，却没有形成current contract，也没有规定live scheduling如何进入普通unit test。

本轮审计发现三类测试开始反向塑造production：MM destructive TLB completion在production transaction中回调KUnit
pause hook，并用固定32次`yield_now()`推断successor不可能继续；Event为一个timeout cancellation case在production
object中保存probe，并把wait helper泛化为test observation callbacks；namei retry helper为mount move case增加KUnit-only
one-shot interleaving hook。这些case表达了有价值的correctness obligation，但其proof mechanism依赖人工暂停点或有限调度
机会，既不构成逻辑完备的并发证明，也让ordinary production形状理解test harness。

## Decision

- KUnit仍默认采用pure/deterministic owner-local case；不是一刀切禁止scheduler、wait、kthread、timer或IPI。
- live scheduling只有在并发机制本身是被测语义时才准入，必须走production lifecycle并以显式phase、predicate、Event、
  wait token、completion或join闭合。固定yield/schedule/tick次数、wall-clock sleep不能模拟happens-before或absence。
- timer/timekeeping、timed-wait与scheduler tick accounting可以把时间/tick作为被测输入；其它timeout只能作为
  failure bound，不能替代readiness或ordering handshake。
- PASS只覆盖实际运行tuple与path；缺少SMP/device而早退的case不形成对应证据，不能借suite总数静默外推。
- production不得保存KUnit state、按test branch或只为测例泛化helper。owner-local conditional fixture constructor和
  non-behavioral observation仍可保留，但不能进入ordinary API、production decision或第二份truth。
- 违反测例只有在production path加自然握手仍保持owner直接形状时才局部修复；否则删除测例和test seam，并同步撤回
  过时proof claim。删除proof mechanism不删除其production correctness obligation。

## Implementation Boundary

**Target:** 建立effective KUnit execution/proof current contract；审计全部当前registered cases，并使cutover时没有已知
`KUNIT-CONCURRENCY-001`、`KUNIT-PROOF-001`或`KUNIT-SHAPE-001`违反；以一个closure checkpoint完成代码与contract
原子切换。

**Non-goals:** 不重新证明每个production subsystem语义；不增加runner timeout/skip/filter/fixture framework；不改变
public API、ABI或production visible behavior；不为删除测例建立替代integration framework。

**Owners / handoff:** KUnit runner与repository validation policy拥有执行/proof规则；各production subsystem继续唯一
拥有被测状态、并发协议、failure和cleanup。case只持production capability及test-local phase，不接管owner state。

**Failure / cleanup:** 普通panic保持terminal；live worker/request/listener仍由case通过production lifecycle闭合。删除case
时同步核对其validation claim；若它是不可替代的required acceptance evidence则停止，不以弱证据宣称closure。

**Protected surface:** kernel/user ABI、production semantics、各subsystem owner/contract与历史执行事实保持不变。
本轮只改变KUnit execution/proof shared contract、conditional test surface及当前validation claim。

**Stop conditions:** 修复要求改变production语义、owner、public API、subsystem contract或acceptance；需要新probe、
transitional contract、runner mechanism或多个cutover；删除测例会留下无法诚实替代的required acceptance gate。

## Contract Impact / Cutover

代码清理与current contract在本closure checkpoint原子生效；checkpoint未完整验证或review通过时不声明cutover。

### Minimum effective baseline extraction

`KUNIT-EXEC-001/002`与`KUNIT-PROOF-001`此前已由2026-08-02 change record和live runner docs生效；本轮只把它们
提取到current contract作为唯一current authority，并明确缺席SMP/device等前提而早退不形成对应proof。它们不是本轮
新引入或替换的语义，不登记虚假的`Introduce`/`Refine`。

| Contract ID | 变化 | Cutover前effective baseline | 新effective规则 | 生效证据 |
| --- | --- | --- | --- | --- |
| `KUNIT-CONCURRENCY-001` | Introduce | None | live scheduling是被测并发语义的窄例外；必须逻辑握手，禁止固定调度/时间机会作为oracle | 全量并发candidate audit及删除违规case |
| `KUNIT-SHAPE-001` | Introduce | None | production state/control flow/API不得理解KUnit；conditional fixture/observation受窄边界约束 | `cfg(kunit)`与hook/probe/caller audit及普通build |

唯一effective正文位于[KUnit Execution and Proof当前契约](../../contracts/kunit/execution-and-proof.md)。

## Change

审计基线为197个Rust文件中的637个registered cases。通过registered-case文件与kthread/yield/schedule/wait/timeout/tick、
SMP topology、`cfg(kunit)`、hook/probe和`for_kunit`交叉扫描，再对候选逐项核对被测对象、握手、failure bound、cleanup、
production caller与proof claim。本轮保留scheduler oneshot、kthread timed wait、kworker、timer/timekeeping、timerfd及
user-TLB IPI等真实并发/时间subject；它们使用production lifecycle与显式phase/Event/token/completion，或只把tick/time
作为被测状态机输入。SMP前提缺席时的早退只保留为case适用性，不计为对应topology proof。

删除7个registered cases；source-level registration inventory由197个文件/637项变为195个文件/630项。实际链接进
某次runner的数量仍由architecture与feature tuple决定，不能把630写成任一runtime总数：

- `mm/uspace/fence.rs`删除5个forced-interleaving cases、全局pause state和production callback。其固定replace、fork、
  permission restriction、heap decommit及monotonic/no-change obligation仍由MM contract与production source review拥有；
  当前不再声称有live forced-interleaving KUnit。
- `sched/event.rs`删除1个early-wake timeout-request case、`Event` probe字段及KUnit branch；`sched/mod.rs`删除只服务该
  probe的generic install/cancel callbacks，恢复直接production wait helper。
- `fs/mount/tree.rs`删除1个move/lookup retry case；`fs/namei.rs`删除KUnit hook和generic one-shot callback，恢复直接
  generation retry loop。历史mount RFC/transaction中的旧运行事实保持不变，不能作为当前test inventory。

`AGENTS.md`、runner module docs和文档导航改为引用同一current contract；不复制第二份完整normative正文。

## Validation

- 全量source inventory与candidate audit覆盖registered-case文件中的kthread/spawn、yield、schedule/wait、timeout/tick、
  SMP topology，以及全仓`cfg(kunit)`、hook/probe和`for_kunit` surface。保留的live并发cases均能指出被测并发owner、
  显式phase/Event/token/completion、failure bound与cleanup；删除后residual search对三个旧hook、probe、generic callback和
  固定32次yield均为零命中。
- `just fmt kernel --check`、`git diff --check`与`mdbook build docs`通过。
- `just build --preset competition-final-rv64-release --bind smp=8 --bind memory=8G`通过。
- `just build --preset competition-final-la64-release --bind smp=8 --bind memory=8G`通过。
- fresh pretest rootfs与worktree-local disk副本上的RV64 `qemu-virt-rv64-release`、SMP=8、8 GiB：实际链接并执行
  609/609 KUnit，出现`All tests passed!`并正常power off。cross-CPU timekeeper、remote realtime timer lane、selected
  user-TLB target和RISC-V remote icache case均真实执行；随后附带的glibc/musl socket smoke各3/3通过。
- 同形LA64 `qemu-virt-la64-release`、SMP=8、8 GiB：实际链接并执行612/612 KUnit，出现`All tests passed!`；
  cross-CPU timekeeper、remote realtime timer lane、selected user-TLB target与global membarrier rendezvous均真实执行，
  随后glibc/musl socket smoke各3/3通过并完成filesystem/network/device shutdown。LA64既有QEMU platform没有成功的
  power-off handler，guest在完整marker后halt，宿主通过QEMU console正常终止；这不外推为hardware power-off证据。
- 最终source/diff audit与Architecture Friction Scan确认没有第二份状态真相、owner穿透、private representation泄漏、
  public surface扩张、遗留test special case或无退出条件桥；本轮反而删除了三条test-driven production seam。
- **Independent review：Not Run。** 维护者在2026-08-11明确取消原定subagent review并授权直接收口提交；本项不被
  替换为内部自审，也不影响上述已完成的source/build/runtime证据范围。

## Remaining Risk / Links

- MM destructive completion、Event timeout cancellation与mount generation retry的production correctness obligation没有被接受
  为限制；本轮只是撤回不可靠/污染production的KUnit proof mechanism。各自当前proof来源必须如实记录为source review、
  历史runtime或其它仍存在的定向测试。
- KUnit仍没有per-case timeout、skip metadata、isolation或recoverable panic；本轮不引入相应runner抽象。
- full LTP、final harness、实体硬件、expected-panic/death-test与性能测试：Not Run。上述双libc socket profile只是
  rootfs自带的窄shutdown smoke，不外推为full LTP或本轮acceptance主体。
- 历史执行边界：[2026-08-02 KUnit execution boundary](./2026-08-02-kunit-execution-boundary.md)。
