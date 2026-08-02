# ANE-CHG-20260802-kernel-developer-logging

**Type:** Small Feature / native developer ABI
**Status:** Completed
**Date:** 2026-08-02
**Authors:** doruche, Codex
**Area:** printk / timekeeper / console replay / native debug ABI

## Problem / Context

原 printk 是一份早期临时实现：record 在入库前已经混入 ANSI 与 level presentation，只有
level 和 fixed bytes，没有 timestamp、callsite、truncation flag 或 sequence；console replay
还直接读取 ring/record 表示。fixed-buffer overflow 可以从 UTF-8 code point 中间截断，随后把
整条消息退化成 `[Invalid UTF-8]`。

日志级别也只有 build-time `RECORD_LOG_LEVEL` / `PRINT_LOG_LEVEL`。即使 Debug 已被编译进
kernel，开发者仍不能在运行时原子调整 record/console filter。native `SYS_DBG_PRINT` 则绕过
结构化 record 直接写 console；live source 中没有真实 caller，只有未被消费的 userspace
wrapper，因此本轮可以在原 native slot 做一次原子 ABI replacement，而不保留双路径。

这些 owner、presentation、UTF-8 和 native ABI 决策值得长期保存，但都能在一个完整解析的
checkpoint 内闭合，不需要 probe、transitional contract 或多阶段 cutover，因此保持为小迭代。

## Decision

printk 是 record、runtime policy、ring sequence 与 presentation 的唯一 owner。同一 owner 内按
稳定职责拆成 `record`、`policy`、`ring` 与 `presentation` 子模块；`mod.rs` 只保留 facade、
宏入口和 composition KUnit。该目录化拆分不建立新的 semantic owner，不扩大 private record
visibility。

每个带 level 的宏调用点使用静态 callsite metadata。record 保存 raw UTF-8 message、callsite、
boot timestamp 与 flags；sequence 只由 ring append 分配，不复制进 record。live output 与 replay
调用同一 printk-owned formatter，console 只提供 writer capability，不读取 ring、record、policy
或 formatter 私有表示。raw level-less `kprint!` / `kprintln!` 继续是 console fragment，不伪造
record metadata。

timekeeper 新增 early-safe snapshot：只有全部 boot CPU 发布本地 baseline 后才允许读取 arch
counter 与当前 CPU 的 per-CPU baseline；此前返回 unavailable。严格的普通 time API 不放宽，
早期 record 也不会在 replay 时补写时间。

`RECORD_LOG_LEVEL` 继续是 compile ceiling。printk 用一个 packed atomic 保存 runtime
`record_level` 与 `console_level`；一次宏调用在求值 format 参数前只 snapshot 一次 policy，因而
不会观察拆分更新，也不会在 disabled callsite 求值参数或进入 ring/console path。

native slot `SYS_ANEMONE_START + 0` 原子替换为 `SYS_DBG_LOG_CTL`：

- `GET_LEVELS(0)` 要求第二参数为零，返回当前 packed levels；
- `SET_LEVELS(1)` 在成功时原子替换两项 level，并返回替换前的完整值；
- bits `0..=7` 是 record level，bits `8..=15` 是 console level，其余 bits 必须为零；
- level domain 是 `Emerg=0 .. Debug=7`，并保持
  `console <= record <= compile ceiling`；
- malformed request 返回 `EINVAL` 且先于权限分类；结构有效的 SET 要求 effective
  `CAP_SYS_ADMIN`，否则返回 `EPERM`；所有拒绝路径保持 policy 不变。

旧 `SYS_DBG_PRINT` handler、常量与两层 wrapper 全部删除，不保留 alias、fallback 或 deprecation
bridge。`anemone-rs` 提供 typed get/set wrapper；真实 smoke 使用
`get -> set -> exercise -> set(old)` 显式恢复，不让 kernel 按 pid 或进程退出猜测 policy owner。

## Change

- printk record 增加静态 callsite、early-safe timestamp、flags 与 UTF-8-safe fixed-buffer writer；
  截断消息保留合法 prefix，并由 formatter附加可见 `[truncated]` marker。
- ring append 返回唯一 sequence，weak iterator交付 `(sequence, record)`；console output 始终在
  ring lock 之外执行。
- compile gate 与 packed runtime gate 移到 `format_args!` 求值前；policy SET 使用完整 word
  atomic swap。
- console replay 改为向 printk 提供 writer capability；live/replay 共用 formatter，默认输出
  timestamp、level 与 `module_path:line`。
- timekeeper 增加全局 publication readiness 与 non-panicking snapshot，但不改变普通
  `monotonic_uptime()` 的严格行为。
- `anemone-abi`、kernel syscall registry 和 `anemone-rs` 共同切换到 `SYS_DBG_LOG_CTL`；新增
  `log-test` 与双架构 focused rootfs/config，覆盖 GET、SET、invalid encoding、permission 与
  restore。
- 所有本轮 KUnit 都是普通 `#[kunit]`。policy concurrency case 使用普通 KUnit 启动普通
  kthread；没有使用 `#[kunit(percpu)]`。

本轮不增加 Linux `syslog(2)` / `dmesg` / `/dev/kmsg` 兼容、ring reader/clear、userspace log
injection、per-task/per-module filter、typed fields/span/subscriber、panic dump或跨 CPU physical
console ordering保证。

## Validation

- 临时把 focused config 的 compile ceiling 从 Debug 降为 Info，并运行 RV64 SMP=8：344/344
  KUnit 全部通过，普通
  `compile_disabled_macro_does_not_evaluate_arguments` KUnit 实际执行并通过。该运行随后按预期因
  smoke 请求超过临时 ceiling 而拒绝 Debug SET，因此只作为低 ceiling macro execution evidence；
  tracked config 已恢复为 Debug，没有保留第二份 policy/config truth。
- 使用 fresh `log-acceptance-rv64` rootfs，以 explicit tuple
  `qemu-virt-rv64` / `conf/kconfs/log-acceptance.toml` / `release`、SMP=8、4 GiB 串行完成 final
  build/boot。344/344 KUnit 全部通过，包括普通 kthread policy concurrency、early timestamp、
  UTF-8 truncation、shared formatter、sequence 与 syscall matrix；`log-test` 打印
  `get-set-invalid-permission-restore ok`，`EINVAL` / `EPERM` 可见且最终 policy 恢复，随后完成
  orderly shutdown 与 SBI PowerOff。
- 使用 fresh `log-acceptance-la64` rootfs，以对应 LA64 explicit tuple、SMP=1、4 GiB 和
  `net-user-options=restrict=on` 串行完成 final build/boot。349/349 KUnit 与同一 userspace
  smoke 全部通过。guest 完成 filesystem/network/device shutdown 后，由于当前 LA64 platform
  没有可用 poweroff handler而进入 halt；terminal PASS 已出现后由 host 退出 QEMU。
- 两架构 boot log 都先显示 `[    ?.??????]` early record，timekeeper ready 后显示 monotonic
  timestamp；prefix包含 level与 callsite，UTF-8 truncation marker可见，没有把 payload写成
  ANSI/prefix，也没有在 replay 中补写 early timestamp。
- RV64 与 LA64 build/runtime严格串行，未把共享
  `build/generated/device-tree/platform.dtb` 的并行结果作为证据。sandbox 内 lwext4 C compile
  曾命中 `Bad system call` / `SIGSYS`；完全相同 canonical build在 sandbox外通过，因此按 host
  environment限制归类。
- `just fmt log-test --check`、`git diff --check`、new-file whitespace check、focused residual
  source audit与 `mdbook build docs`通过；production code/manifests中没有旧 `SYS_DBG_PRINT` /
  `dbg_print`，本轮 KUnit中没有 `#[kunit(percpu)]`。`just fmt kernel --check` 的剩余差异全部位于
  既有 vendored `anemone-kernel/crates/anemos/virtio-drivers/`，本轮 kernel-side files没有新增
  formatter drift。
- 一位独立 reviewer 的 change review先发现并推动修复 console读取 printk private
  representation、smoke扩大通用 `setuid` API、以及低 compile ceiling缺少执行证据；同一 reviewer
  复核确认三项均关闭，没有新的 Apollyon、Keter 或 Euclid finding。
- Hardware、完整 user-test/LTP profile、panic-context logging、ring extraction UAPI 与 physical
  multi-console ordering均 **Not Run**；双架构 focused QEMU不能外推这些能力。

## Remaining Risk / Links

- printk/console仍使用 ordinary locks；panic-time logging、console reentrancy与 terminal failure
  保持既有 best-effort边界。本轮没有把 sequence顺序外推为跨 CPU physical emission顺序。
- sequence与结构化 metadata当前服务 ring/replay/tests，尚无 userspace reader。未来 reader需要
  独立解析 cursor、overwrite、blocking、权限与 ABI，不能直接公开当前 private record表示。
- repository-wide kernel formatter仍受既有 vendored `virtio-drivers` drift阻断；这不是本轮 source
  或 runtime acceptance的通过项。
- Current contract / register / limitation: None；本轮 native ABI与 owner/handoff/failure/cleanup在
  一个原子 small-change checkpoint中闭合，没有修改其它 effective shared contract或当前登记项。
- RFC / transaction / external source: None
- Issue / PR / commit: this change's Git commit
