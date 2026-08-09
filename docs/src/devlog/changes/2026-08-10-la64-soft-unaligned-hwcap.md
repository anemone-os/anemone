# ANE-CHG-20260810-la64-soft-unaligned-hwcap

**Type:** Small Feature / Linux ABI compatibility
**Status:** Completed
**Date:** 2026-08-10
**Authors:** doruche, Codex
**Area:** LoongArch64 / execve / ELF auxv / software unaligned access

## Problem / Context

LoongArch QEMU的TCG host backend要求`AT_HWCAP`包含`HWCAP_LOONGARCH_UAL`，否则会在初始化时拒绝运行。
Anemone此前始终把`AT_HWCAP`序列化为0，即使LA64 kernel已经通过`soft_unaligned_access`为用户态提供
非对齐访问fallback，也无法把这一能力告知QEMU等consumer。

本轮需要保存一个局部ABI取舍：Anemone把“用户态可以执行非对齐访问”作为UAL capability，不要求auxv区分
该能力由CPU硬件还是kernel trap模拟提供。软件模拟自身的正确性和性能仍由LoongArch unaligned handler拥有，
不进入本轮auxv发布的acceptance。

## Decision

- 仅在`target_arch = "loongarch64"`且编译启用`soft_unaligned_access`时发布Linux
  `HWCAP_LOONGARCH_UAL` bit 2；其它架构和关闭feature的LA64 build继续发布0。
- `AT_HWCAP`表达用户态可消费的能力，不承诺硬件实现、访问成本或软件模拟器不存在缺陷。
- ELF auxv owner只消费build-time feature handoff并序列化mask；不引入CPUCFG探测、per-CPU capability snapshot、
  QEMU/platform特判或通用feature publication framework。

## Implementation Boundary

**Target:** 使启用`soft_unaligned_access`的LA64 ELF进程在`AT_HWCAP`中观察到
`HWCAP_LOONGARCH_UAL`，其它现有auxv entry和值保持不变。

**Owners / handoff:** LoongArch user-trap owner通过kernel feature决定是否提供软件非对齐访问；ELF auxv owner只把
该build-time capability handoff编码为Linux UAL bit。软件模拟的指令解码、用户内存访问、失败和寄存器提交均不由
auxv owner接管。

**Failure / cleanup:** 本轮不增加运行期状态、分配、失败或cleanup路径。feature关闭时不发布UAL；未知或未支持的
其它HWCAP继续为0。

**Protected surface / Contract Impact:** RV64、`AT_HWCAP2`、其它auxv entry、软件非对齐trap行为、current contract与
LSX feature publication limitation均保持不变；`Contract Impact`为`None`。本轮不关闭软件模拟内存破坏问题，也不
声明LA64 QEMU runtime已经通过。

## Change

- 增加`elf_hwcap()`，按architecture与feature编译条件生成UAL mask；注释固定软件实现也满足本轮UAL capability的
  ABI选择及责任边界。
- `AuxEntry::HwCap`改为直接携带mask，消除`NotSupported` marker与非零序列化结果之间的矛盾；partial auxv构造时
  取得一次build-static mask，serialize只编码entry自身的值。

## Validation

- `just fmt kernel --check`：通过。
- `git diff --check`：通过。
- `mdbook build docs`：通过。
- `just build --preset competition-final-la64-release --bind smp=8 --bind memory=8G`：通过；selected KernelConfig启用
  `soft_unaligned_access`，形成LA64 compile/link证据。
- source review确认LA64 + feature分支只返回bit 2，其余编译分支返回0；`AuxV::new_partial()`始终携带并序列化该mask。
- **Not Run:** QEMU、auxv用户态oracle、KUnit、LTP、final harness、2K1000实体硬件与软件非对齐stress。

## Remaining Risk / Links

- [`ANE-20260801-LA64-SOFT-UNALIGNED-USER-MEMORY-CORRUPTION`](../../register/open-issues.md#ane-20260801-la64-soft-unaligned-user-memory-corruption)
  保持Open。该缺陷属于被发布能力的实现owner，不改变本轮auxv capability handoff；本轮build证据也不证明软件模拟
  runtime正确。
- [`ANE-20260801-LA64-LSX-STICKY-LAZY-SCOPE`](../../register/current-limitations.md#ane-20260801-la64-lsx-sticky-lazy-scope)
  保持Active；本轮只发布UAL，不发布LSX、LASX、LBT或通用CPU feature mask。
- Current contract / RFC / transaction：None。
- 外部源码：`xref:linux-6.6.32:arch/loongarch/include/uapi/asm/hwcap.h#HWCAP_LOONGARCH_UAL`。
