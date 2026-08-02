# ANE-CHG-20260802-kunit-execution-boundary

**Type:** Cleanup / test framework boundary
**Status:** Completed
**Date:** 2026-08-02
**Authors:** doruche, Codex
**Area:** KUnit / boot / kthread / IPI / validation

## Problem / Context

KUnit 已经从纯函数测试扩展到 VFS、设备、调度、wait、IPI 和普通 kthread，但 runner 与
`#[kunit]` 只说明测试签名，没有定义启动时点、current task 能力、可用 provider、共享状态、
cleanup 或验证外推边界。历史上 truncate KUnit 曾把 BSP boot task 当作普通 task 做路径解析，
命中 hanging `FsState` 并 panic；近期也出现过不触达 production transaction、却被当成实现证明的
synthetic scheduler KUnit。

框架还保留没有 consumer 的 `#[kunit(percpu)]`。它通过 test-only IPI payload 让同一个任意
`fn()` 在各 CPU 的硬中断路径中执行，并以全局自旋 barrier 协调。这个机制把测试策略写入 IPI
owner，隐藏中断、同步和失败上下文，也不符合当前用普通 kthread 或 production IPI path 构造并发
证明的方向。

这些局部决策会长期影响跨 subsystem 的测试形状，但 KUnit owner、failure、cleanup 和验证边界能在
一个原子 checkpoint 中闭合；本轮不需要 probe、production contract cutover 或多阶段迁移，因此按
小迭代处理。

## Decision

KUnit 是 boot-integrated kernel test runner，不是用户进程 sandbox：

- case 在 BSP `kinit` task 上串行运行；全部配置 CPU 已完成 local init，Late initcall、设备 attach、
  boot I/O finalize 与 rootfs mount 已完成，interrupt、scheduler、timer、allocation 和 `kthreadd`
  可用；
- initial userspace task 尚未 prepare。current task 只保证显式 kernel-task/scheduler 能力，不保证
  普通用户进程的 user memory、files、`FsState`、credentials、signals 或 userspace trap frame；
- case 共享 live kernel、global registry 与 rootfs，不保证隔离或执行顺序。成功返回前必须撤销
  publication、恢复全局状态、删除 filesystem fixture，并 stop/join 自己创建的全部 kthread；
- owner-local 测例可为明确接受 detached task 的 API 构造未 publish 的 `Task` data fixture；可以在
  test-local scheduler object 中使用，但不得 publish 到 global task topology、enqueue 到 live processor
  或执行，也不能用它证明 task topology、调度执行或完整 lifecycle；
- 需要实际运行的 kernel concurrency 只通过 production `KThreadBuilder` 生命周期构造；KUnit 环境不支持
  spawn userspace task。SMP 测试通过 pinned kthread、scheduler 或 production IPI API 显式构造拓扑，
  不执行 per-CPU test function；
- PASS 只证明本次实际 architecture、CPU count、device、filesystem 与 feature tuple，不能从
  SMP=1 外推 SMP，也不能从 QEMU 外推 hardware；
- semantic negative case 继续用普通 Rust assertion 验证 `Err`、`None` 或拒绝转换。任何 panic 都
  终止 kernel 和本次 run；runner 不 unwind，也不在可能已部分修改的共享状态上继续执行。预期 panic
  需要独立 single-case boot 与 host terminal oracle，不属于普通 KUnit。

本轮不增加 `EXPECT`/`ASSERT` 宏家族、fixture framework、per-test task、用户 task spawn、death-test
runner、过滤器或自动 skip/requirements metadata。标准 `assert!` / `assert_eq!` / `matches!` 已经表达
fatal assertion；只有出现真实重复 consumer 后，才考虑 bounded progress 等窄 helper。

## Change

- `debug::kunit` 的 module documentation 成为 live execution contract；runner 保持一个简单的串行
  `fn()` registry，并删除 catch-panic TODO。
- `#[kunit]` 恢复为无参数 attribute；带参数用法在编译期明确拒绝。
- 删除 `KUnitKind::PerCpu`、全局 per-CPU barrier/guard、IPI handler test executor，及
  `IpiPayload::RunKUnitPerCpu` 和对应 broadcast-copy KUnit 分支。production IPI payload/transport
  语义不变。
- source audit 确认当前没有 `#[kunit(percpu)]` consumer。scheduler owner 的 KUnit 会构造未注册
  `Task` fixture，但都调用 `PublishGuard::forget()` 保持 detached，没有进入 global task topology 或
  live processor；它们只覆盖 detached scheduler object/API，不证明 task spawn 或 execution。现有真正
  运行普通 kthread 的 KUnit consumer 位于 TTY worker、scheduler oneshot 和 printk policy：它们都复用 production
  `KThreadBuilder`；成功路径分别通过 attachment abort 或 `wait_exited()` 完成 worker cleanup，并保持
  owner-local fixture/global-state restore。
- 不修改 register/current limitations 或 current contract：本轮没有接受新的 production 缺口，
  也没有改变 kernel ABI、用户可见语义或 production owner。

## Validation

- `just build --preset qemu-virt-rv64-release --bind smp=1 --bind memory=1G` 与
  `just build --preset qemu-virt-la64-release --bind smp=1 --bind memory=1G` 串行通过。
  RV64 在 sandbox 内编译 `lwext4` C source 时出现 `Bad system call` / `SIGSYS`；相同命令在
  sandbox 外通过，故分类为 validation environment 限制，不是 kernel build failure。
- RV64 QEMU、SMP=1、pretest ext4 rootfs：`344/344` KUnit 通过，打印
  `All tests passed!`，随后完成 orderly power-off 并由 QEMU 正常退出。
- LA64 QEMU、SMP=1、pretest ext4 rootfs：`349/349` KUnit 通过，打印
  `All tests passed!`。随后完成 userspace 小集合与 shutdown callback，但当前 LA64 没有成功的
  power-off handler，guest 进入既有永久 halt；宿主只在完整 KUnit marker 之后终止 QEMU。
- `git diff --check`、新 change record 的独立 whitespace check 与 `mdbook build docs` 通过。
- `just fmt kernel --check` 的剩余 diff 全部位于既有 vendored
  `anemone-kernel/crates/anemos/virtio-drivers/`；本轮修改的 Rust 文件没有 formatter diff。
- SMP>1 KUnit、硬件、expected-panic/death-test oracle、完整 LTP/competition suite：Not Run。

## Remaining Risk / Links

- KUnit 仍没有 per-case isolation、runner-level timeout、filter 或 skip。死锁由 active case name、
  panic/host timeout 和 focused source audit 定位；本轮不通过把 case 放进可遗留的 worker sandbox 来
  伪造可恢复性。
- 现有全部 KUnit 的 production-proof 适用性仍由各 subsystem owner 与对应 validation claim 负责。
  本轮只审计 task/kthread/per-CPU 环境边界及当前真实 consumer，不把 300 余项测例重写成中心化
  framework ownership。
- Current contract / register / limitation: None
- RFC / optional transaction / external source: None
- Issue / PR / commit: this change's Git commit
