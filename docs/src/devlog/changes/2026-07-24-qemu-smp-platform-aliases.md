# ANE-CHG-20260724-qemu-smp-platform-aliases

**Type:** Small Feature
**Status:** Completed
**Date:** 2026-07-24
**Authors:** doruche, Codex
**Area:** build system / configuration / QEMU Platform

## Problem

QEMU RV64与LA64的generic/pretest Platform各自复制一份machine配置，但差异实际是workload用途与预期CPU数，
不是两种不同machine model。原system-target-model R4把这项配置应用列为R4B Outline，扩大了模型RFC的
acceptance boundary。

同次清理发现resolver、Platform、DT、app和rootfs单测直接枚举或读取production配置。配置文件是可动态增删的
repository input，不应由测试代码再维护一份“supported”清单；这种耦合会让配置变化变成无关测试失败并产生偏移。

## Scope

本轮建立两架构的SMP1/SMP8 QEMU Platform配置和现有用途名的符号链接，并把配置测试统一到各层canonical
example。保持SystemTarget、BuildPreset、wrapper、root/initial-program、KernelConfig、production resolver、
schema、kernel与final harness行为不变。R4B从RFC移除，但R4A DT authority/delivery、normal-build
materialization与DT-neutral bind规则保持。

## Solution

每架构建立`qemu-virt-<arch>-smp-{1,8}.toml`两份canonical配置。SMP1/SMP8分别固定`smp = 1/8`与
`max_phys_cpu_id = 0/7`；四份配置都声明`kernel-image`、`disk-x0`、`disk-x1`三个bind。现有
`qemu-virt-<arch>-pretest.toml`符号链接到SMP1，现有无用途后缀的`qemu-virt-<arch>.toml`符号链接到SMP8，
从而保持所有SystemTarget、BuildPreset与wrapper引用不变。

`-no-reboot`不是guest hardware fact，但Platform manifest也拥有持久的QEMU fixed argv，而不只是硬件常量；
因此四份canonical配置都直接保留该参数。本轮不为它增加统一execution-policy字段或新的owner surface。

Resolver测试把`conf/{build-presets,system-targets,platforms}/example.toml`与`conf/.defconfig`复制到临时
workspace，只在临时副本上验证缺失输入、fallback和immutable snapshot。Platform/DT测试读取
`conf/platforms/{example.toml,example.dts}`并派生所需contract变体，不读取任何具体QEMU或物理Platform。
Rootfs示例从`minimal.toml`改名为`example.toml`，rootfs parser测试也只读取该example；不再读取具体pretest
manifest。App parser测试只读取`conf/app.toml`并从该example派生Source变体，不再遍历production app manifests。
仓库不保留测试专用配置fixture或第二份Kconfig truth。

## Change

- 新增RV64/LA64各一份SMP1与SMP8 canonical Platform；
- 把四个既有Platform路径改为相对符号链接：pretest -> SMP1，generic -> SMP8；
- 删除resolver内的production target/preset枚举测试和测试专用Kconfig，统一读取canonical examples；
- 新增`conf/platforms/example.dts`，将`conf/rootfs/minimal.toml`改名为`example.toml`；
- 将system-target-model提升为R5并关闭，删除未激活R4B的实施入口；
- 更新配置说明、RFC导航、双周devlog和小迭代导航。

## Validation

- `just build --preset qemu-virt-rv64-release`通过，解析`target/platform = qemu-virt-rv64`并消费SMP8配置；
- `just build --preset qemu-virt-la64-release`通过，DT materializer执行
  `qemu-system-loongarch64 ... -smp 8 -m 1G`后完成kernel release build；
- RV64首次沙箱内构建在lwext4 C compile处因seccomp `SIGSYS`失败；同一仓库命令在沙箱外重跑通过；
- `just xtask-test`通过，52项测试全部成功；
- `just fmt all --check`、`git diff --check`与`mdbook build docs`通过。

未运行QEMU guest、SMP runtime、pretest/final harness、LTP或physical hardware；双平台build不外推为这些证据。

## Tracking Issues

None.

## Risk / Follow-up

现有引用名仍是兼容别名，resolver诊断显示别名identity而不是符号链接目标名。若未来需要让hardware identity
直接进入`ResolvedSystemBuild`，应单独迁移SystemTarget reference和consumer；测试不再拥有production inventory。
本轮不为显示层纯化修改resolver。
Final runner、root/initial-program选择和赛方镜像兼容仍属于独立adopter工作。

## Links

- Biweekly devlog: [2026-07-20 至 2026-08-02](../2026-07-20_to_2026-08-02.md)
- Current contract: None（[`BOOT-PROTOCOL-001`](../../contracts/task/boot-protocol.md) Preserve）
- Register / limitations: None
- RFC: [System Target Model R5](../../rfcs/system-target-model/index.md)
- Historical transaction: [R4A QEMU Provider DT Cutover](../transactions/2026-07-24-system-target-model-r4-qemu-dt.md)
- 外部源码证据：无
- Issue / PR / commit: None
