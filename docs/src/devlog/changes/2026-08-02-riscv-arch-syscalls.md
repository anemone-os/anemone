# ANE-CHG-20260802-riscv-arch-syscalls

**Type:** Small Feature / Linux ABI Compatibility
**Status:** Completed
**Date:** 2026-08-02
**Authors:** EDGW, Codex
**Area:** RISC-V / syscall ABI / CPU capability projection / instruction cache

## Problem / Context

Anemone 的 RV64 syscall 表缺少 Linux RISC-V 专属的 `riscv_hwprobe(2)` 258 和
`riscv_flush_icache(2)` 259。现有 CPU discovery 只用 `riscv,isa` 做准入，不保存每个
hart 的完整 capability/ID snapshot；直接把调用 hart 的 SBI ID 或 ISA 字符串投影到任意
CPU mask 会虚报能力。另一方面，用户态单独执行 `fence.i` 不能覆盖线程迁移后的远端 I-cache。

## Decision

本轮采用保守、ABI 诚实的局部能力：

- `riscv_hwprobe` 实现 Linux 6.6 的 flags、CPU mask、逐 pair copy 和 unknown-key 规则；
- SBI ID key 暂按 unknown key 返回，不伪造 selected CPU set 的一致 ID；
- 只报告 CPU 准入规则已保证的 IMA、F/D 和 C，misaligned performance 返回 unknown；
- 不报告 Vector、Zba/Zbb/Zbs，也不建立 per-CPU capability snapshot；
- `riscv_flush_icache` 忽略 Linux 当前保留的 address range，校验 bit 0，并以本地
  `fence.i` 加 SBI RFENCE 同步刷新全部在线 hart；`LOCAL` 暂采用更强的同样语义。

ID key 返回 unknown 是明确的兼容缩减，因此 Linux 6.6 hwprobe selftest 中要求 key 0..3
全部被识别的断言不属于本轮 acceptance；其它参数、unknown-key 和保守 capability 语义仍需验证。

## Change

- `anemone-abi` 在独立 `hwprobe` 模块增加 Linux `riscv_hwprobe` pair 布局和本轮使用的
  key/bit，在 RV64 syscall 模块只增加 syscall 号。
- RV64 arch owner 的 `api` 模块增加两个 syscall adapter；LA64 不注册 258/259。
- hwprobe 按 key/value 字段分别访问用户内存，保留 Linux key 先于 value 的部分 copy 行为。
- CPU owner 一次性派生准入规则保证的 IMA extension flags；SBI RFENCE availability 首次使用时
  查询并记录结果，后续 flush 复用该结果。
- 定向 KUnit 覆盖保守 key 投影、CPU mask 在线交集和 flush flags。

## Validation

- `just fmt kernel` 通过。
- `just build --preset qemu-virt-rv64-release --bind smp=1 --bind memory=2G` 通过。
- `just build --preset qemu-virt-la64-release --bind smp=1 --bind memory=2G` 通过，确认
  LA64 不注册 RISC-V 258/259。
- 增加远端生产函数 KUnit 后，RV64 SMP2 board Kconfig 运行 323 项 KUnit 全部通过；
  `test_riscv_flush_icache_reaches_online_harts` 在两个在线 hart 上完成 SBI RFENCE。
- 最终单核正式 preset 的 323 项 KUnit 再次全部通过。本轮不在 `user-test` 内增加专属测例。

## Remaining Risk / Links

- 当前限制：[RV64 hwprobe conservative capability scope](../../register/current-limitations.md#ane-20260802-rv64-hwprobe-conservative-capabilities)
- External source evidence：xref source `linux-6.6.32`（commit
  `91de249b6804473d49984030836381c3b9b3cfb0`）的 RISC-V syscall、cacheflush 与 UAPI 实现。
- Contract Impact：None；本轮没有提取或改变跨 RFC current contract。
