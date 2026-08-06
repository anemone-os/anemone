# ANE-CHG-20260806-user-access-typed-copy-soundness

**Type:** Correctness repair / Rust representation cutover
**Status:** Completed
**Date:** 2026-08-06
**Authors:** doruche, Codex
**Area:** syscall / user access / typed copy / `anemone-abi`

## Problem / Context

`UserReadPtr<T>::read()`与`UserWritePtr<T>::write()`原本都以`T: Copy`作为typed copy的能力
证明，但`Copy`既不保证任意用户字节都是合法`T`，也不保证copyout读取的完整object representation没有
未初始化padding。可达的`SysInfo`copyout同时包含内部和尾部隐式padding，因此generic safe API可能在copyin
形成无效Rust值，或在copyout读取并泄漏未初始化内核字节。

`anemone-abi`中的若干Linux ABI字段还直接使用Rust raw pointer。用户提供的字段在完成user-range验证前只是
64-bit地址表示，不应被制造成带provenance、看似可由内核解引用的pointer；为迁就typed copy另建同形
`FooWire`又会形成第二份ABI layout真相。

## Decision

- scalar copyin只接受`zerocopy::FromBytes`，证明任意完整初始化的输入bit pattern均为合法值；scalar与slice
  copyout只接受`IntoBytes + Immutable`，证明完整表示可安全读取且不含隐式padding。
- typed slice copyin额外要求`FromBytes + IntoBytes + Immutable`：既保证替换后的元素有效，也保证连续byte
  view不会经过padding。pointer/range construction本身不再暗示元素具备typed-copy能力。
- kernel与ABI统一精确使用`zerocopy = 0.8.42`；不允许逐ABI类型手写unsafe trait实现，也不保留`T: Copy`
  fallback。
- `anemone-abi`继续是唯一wire representation。ABI raw pointer字段改用透明`RawUserAddr64(u64)`；它只提供
  raw bits/null转换，不提供`Deref`或内核解引用路径。
- 普通copyout结构使用显式reserved字段闭合padding。`siginfo_t`的`_sifields`与LA64 FP/LSX payload改为唯一
  byte-backed representation和窄codec；storage保持原union的size与8-byte alignment，setter负责清零inactive
  bytes。

## Implementation Boundary

syscall user-access owner唯一拥有用户范围检查、异常支持的byte transfer、typed-copy capability与copy fault映射；
`anemone-abi`唯一拥有wire size、alignment、offset和字段表示；各syscall owner继续拥有flags、variant、range、
domain translation与errno policy。zerocopy trait只证明Rust representation条件，不取得ABI或syscall policy
所有权。

本轮保护syscall number、binary layout、有效字段语义、errno、partial/exact copy与commit ordering、signal
frame/restore协议、socket message progress和VFS direct-user-I/O语义。允许收紧仓库内Rust source API，并把此前
未确定的reserved/padding bytes规范为零；没有改变current contract，也没有把未消费的`bindings.h`纳入source
compatibility承诺。

## Change

- `UserReadPtr` / `UserWritePtr`把construction impl与typed operation impl分离。scalar copyin集中在一个
  `MaybeUninit::zeroed()` byte view中；只有exact copy成功后才`assume_init()`。byte-only partial I/O继续通过
  独立入口保留原progress语义。
- 增加`RawUserAddr64`并迁移`MsgHdr`、`IoVec`、signal action/stack/ucontext、siginfo、robust futex list等实际
  wire pointer字段；socket、signal、job-control、TTY、float、flock、fcntl与epoll测试consumer同步迁移。
- `SysInfo`、signal/ucontext、loop ioctl、SysV IPC及其它production typed caller所用结构获得实际方向需要的
  zerocopy derive和显式padding；`SigInfoFields`与LA64 `FpContextPayload`使用单一byte codec，不保留平行wire
  struct或union truth。
- 增加owner-local KUnit，编译期确认代表类型的方向能力，拒绝`bool` copyin与隐式padding copyout，并验证raw
  address token只承载bits。增加双架构size/alignment/关键offset assertions。
- 更新全部tracked `anemone-abi` consumer lockfile，避免新增依赖令locked build失效或在首次普通构建时污染
  worktree。

## Validation

- `just fmt all --check`、`git diff --check`通过；residual audit确认production typed-copy没有`T: Copy`
  soundness后门、没有手写unsafe zerocopy impl、没有平行wire type，ABI raw pointer只剩`SIG_DFL` / `SIG_IGN`
  用户态常量和pointer-to-bits便利转换。
- `just build --preset qemu-virt-rv64-release --bind smp=1 --bind memory=1G`与对应
  `qemu-virt-la64-release`均完成discovery/final pass、symbol table verification与postbuild。alignment review修复
  后两条命令已重新执行并通过。
- `socket-test`、`signal-test`、`jobctl-test`、`tty-test`、`float-test`、`flock-test`、`fcntl-test`与
  `epoll-test`均在RV64/LA64完成app build；其余受依赖变化影响的tracked app locks通过RV64 owner wrapper构建
  闭合，全部`anemone-abi` consumer lock可用locked/offline dependency resolution。
- focused RV64单HART运行中，445/445 KUnit通过；新增typed-copy capability与raw-address测试均通过。
  `UDPMSGTST`的header/iovec admission、send transaction、recv scatter与output-fault order等7项全部通过。
- 同一focused runtime在glibc与musl下均通过`rt_sigqueueinfo01`、`sigaltstack01/02`和`sysinfo01/02`；
  `sysinfo03`因`CONFIG_TIME_NS`未启用而TCONF。运行的完整signal profile每套libc仍报告32 passed、5 failed、
  3 skipped，因此该运行只作为上述代表路径的smoke，不声明signal LTP全绿。
- 独立change review发现并闭合两个问题：byte-backed类型曾丢失旧union alignment；17个tracked consumer locks
  曾遗漏新依赖。最终review确认无残留Apollyon、Keter、Euclid或blocking finding，且无需新增current contract。
- **Not Run:** LA64 runtime、full LTP、final harness、SMP stress、physical hardware与未选中的syscall runtime。

## Remaining Risk / Links

- runtime smoke不替代编译期全caller proof；反向地，双架构build/layout proof也不替代未运行架构的signal或
  message round trip。上述Not Run边界保持有效。
- focused signal profile中的非目标失败没有被本轮提升为typed-copy closure证据或接受限制；后续若要关闭这些
  signal语义问题，应在其owner下独立复现和分类。
- current contract：None。本轮只收紧owner内部capability与Rust source API，Linux-visible ABI、owner、handoff、
  errno和current semantics均未变化。
- Register：[`ANE-20260805-USER-ACCESS-TYPED-COPY-SOUNDNESS`](../../register/open-issues.md#ane-20260805-user-access-typed-copy-soundness)
  已由本记录、代码、验证与独立review共同中和。
