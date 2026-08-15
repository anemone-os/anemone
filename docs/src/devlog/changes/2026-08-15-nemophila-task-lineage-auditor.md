# ANE-CHG-20260815-nemophila-task-lineage-auditor

**Type:** Small Feature / Framework feedback
**Status:** Completed
**Date:** 2026-08-15
**Authors:** doruche, Codex
**Area:** Nemophila / task lifecycle / WIT / module SDK

## Problem / Context

Nemophila R0已经用一个clone observer证明单point module的load、registration、callback、trap containment与
lifecycle闭环，但尚未由第二个真实consumer证明同一instance组合多个typed weave point。现有WIT world与Rust SDK
export macro也固定理解clone callback；继续逐callback堆叠全局固定export会迫使无关module携带dummy callback，并把首位
consumer的形状固化为整个module envelope。

本轮以`task-lineage-auditor`作为第二个真实module：它观察clone与用户thread开始退出两个task-owned事件，在guest内关联
creator/child TID并输出诊断日志。目标是让真实consumer持续反馈point、WIT/SDK与Host composition形状；不是建立新的task
lifecycle truth或完整tracing产品。

## Decision

- 新增fanout `thread-exit-begin` weave point。它只表示一个用户task已经进入不可返回的`kernel_exit()`路径，携带TID以及
  operation-local normal-exit/signal reason；它不表示ThreadGroup已经`Exited`、可以wait/reap或owner-local cleanup已经完成。
- task exit owner在开中断、可睡眠且未持task-private guard的入口窗口形成值快照；callback结果、trap、poison或缺少binding
  均不改变退出原因、cleanup、topology、parent notification或scheduler zombie handoff。
- `task-lineage-auditor`在一个instance中原子注册clone与thread-exit point。guest-local关联表只是可能不完整的诊断投影，
  不参与kernel行为；runtime仍唯一拥有binding、in-flight、poison与retirement。
- WIT继续是module-visible接口唯一真相。SDK只为已经出现的lifecycle-only、single-clone与clone+thread-exit三种真实world提供直接export
  组合；不引入dummy callback、字符串point ID、动态provider、通用事件总线或未来组合框架。
- 本工作是一个closure checkpoint的小迭代。owner、handoff、failure、cleanup与acceptance均可在现有task/Nemophila边界内
  局部闭合，不重开已Closed的Nemophila RFC。

## Implementation Boundary

task clone/exit owner分别拥有point identity、typed context、WIT lowering与semantic call site；Nemophila generic weave/runtime
继续拥有catalog、transaction-local registration、atomic publication、cohort selection、per-instance serialization、in-flight、
poison和retirement；module只拥有instance-local诊断状态与日志内容。

本轮允许增加thread-exit provider、WIT interface/world、SDK typed registration/export、Host composition、auditor module、
owner-local定向测试，以及在验证期间临时选择auditor为RV64 final-shell embedded module。临时SystemTarget/KernelConfig选择在
运行后必须删除，不形成production配置、probe API或第二条load path。

本轮保护native syscall ABI、exit/exit_group结果、ThreadGroup lifecycle与waitable publication、clone visible semantics、
Nemophila management ABI、artifact/admission/runtime lifecycle及既有clone-only module source surface。non-goals包括exec observer、
decision-returning callback、async queue、fuel/timeout、task handle、字符串/路径context、新Host service、procfs module state、
LA64 runtime、LTP、full final-harness acceptance与硬件证明。

如果实现要求把exit或runtime状态移交给另一owner、让callback影响退出、扩大management ABI、建立平行binding/lifecycle truth、
保留测试专用production path，或降低fresh artifact/真实callback oracle，必须在cutover前停止并重新分级。

## Change

- task exit owner新增fanout `thread-exit-begin` provider，并在`kernel_exit()`首个无锁、开中断窗口提交TID与operation-local
  `ExitCode`快照；Host composition显式安装第二个typed point。
- WIT将首个consumer固化的单一world拆为lifecycle-only、clone-only和clone+thread-exit三种真实module shape；Rust SDK增加
  task-lifecycle bindings、typed `ThreadExitEvent`，并以一个instance-local `CallbackSlot<E>`收口两个point原先会重复的
  pending/registered/drop协议。单点world明确命名为`clone-module`，没有为尚不存在的外部consumer保留旧`module` alias。
- 新增`task-lineage-auditor`：一个load transaction原子注册两个point，guest-local `BTreeMap<child_tid, creator_tid>`只形成
  matched/unmatched诊断投影；module未进入任何SystemTarget长期选择。
- 实现反馈还暴露了logging SDK只投影四个severity、且无法表达kernel raw `kprint!/kprintln!`。WIT/Host/SDK现完整支持八级
  structured write，并在保持既有四个Core Wasm discriminant不变的前提下追加缺失等级；同时增加value-only raw
  `print/println`与SDK格式化macro。auditor以Notice记录事件，并由注册行真实消费raw `kprintln!`。
- `boot-reject-validation`改用lifecycle-only world，不再为没有callback的artifact伪造clone export；
  `just test nemophila-module`纳入auditor fresh build。

## Architecture Friction

本轮消除了三项具体摩擦：固定clone world造成dummy callback、每新增point复制一套unsafe guest callback状态机、logging能力不足
迫使验证修改全局filter。最终形状没有第二份task/runtime状态、module-specific kernel branch、private representation泄漏、测试专用
load path或长期配置；raw输出明确保持console-only，不伪装record。未发现需要另立owner/contract或升级RFC的未决摩擦。

## Validation

- `just fmt kernel --check`与`just fmt modules --check`通过。
- `just module build boot-reject-validation`、`just module build clone-observer`、
  `just module build task-lineage-auditor`均从fresh candidate成功导出，覆盖无callback、单point与multi-point三种world。
- `just build --target qemu-virt-rv64-final --kernel-config conf/kconfs/default.toml --profile release`通过；同一选择下
  `./scripts/run-final-test-rv64.sh etc/final/images/sdcard-rv.img build/task-lineage-auditor-rv64.log`完成真实RV64纵切。
- 纵切只临时在final target内嵌auditor，保持默认console/record level 5。boot中655项KUnit全通过；raw `kprintln!`输出注册行；
  shell TID 148创建TID 149与150，随后分别得到`value=0/7 matched=true creator=148`的Notice exit记录；
  `AUDITOR_SHELL_OK`证明callback后shell继续可用。最终通过QEMU console quit结束VM，无残留QEMU进程。
- 运行后删除临时SystemTarget选择；`conf/kconfs/default.toml`与`conf/system-targets/qemu-virt-rv64-final.toml`均无最终diff。
  随后以恢复后的final target重跑repository build，`nemophila_defs.rs`重新生成空embedded catalog，未留下ignored generated选择。

Not Run：LA64 runtime、LTP、硬件、并发/压力矩阵、完整final-harness acceptance。BusyBox `poweroff -f`所需reboot syscall仍为
相邻NYI，因此不作为本轮shutdown oracle，也不归因于Nemophila。

## Remaining Risk / Links

- Contract Impact：Refine `NEMOPHILA-HOST-001`与`NEMOPHILA-WEAVE-001`；Introduce
  `NEMOPHILA-THREAD-EXIT-001`。上述source、fresh build与RV64 callback证据闭合后已cut over。
- Dependencies：[`NEMOPHILA-RUNTIME-001` / `NEMOPHILA-WEAVE-001`](../../contracts/nemophila/index.md)、
  [`TASK-LIFE`](../../contracts/task/thread-group-lifecycle.md)。
- Historical source：[`RFC-20260814-nemophila`](../../rfcs/nemophila/index.md)只作为R0 provenance，不承载本轮状态。
- Register：当前无本轮新增项。
