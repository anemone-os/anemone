# 2026-07-22 - System Target Model

**Status:** Active
**Owners:** doruche, Codex
**Area:** build system / configuration / platform / repository workflow
**Canonical Plan:** [RFC-20260722-system-target-model](../../rfcs/system-target-model/index.md), [目标与不变量](../../rfcs/system-target-model/invariants.md), [迁移实施计划](../../rfcs/system-target-model/implementation.md)
**Canonical Revision:** R0
**Current Phase:** Stage 1 Active / Checkpoint 1A Closed / Checkpoint 1B Not Started

## Scope

本事务执行R0的滚动实施。当前授权只覆盖Stage 1前两个checkpoint：1A建立dormant SystemTarget
schema、typed reference与loader，1B完成single resolver snapshot和build consumer cutover。每个
checkpoint必须按canonical implementation plan独立review、验证、回写和提交；1A关闭不自动启动1B，
1B关闭也不进入1C。

## Contract and register boundary

本Stage不执行current-contract cutover。`BOOT-PROTOCOL-001` R0 Refine保持pending successor，现有
rootfs metadata到ordinary `kernel_execve()`的effective baseline继续生效。2026-07-23 preflight读取
register、open issues与current limitations，未发现与Stage 1冲突的active build/boot issue。

## R0 acceptance and Stage 1 activation - 2026-07-23

用户明确授权完成Stage 1前两个checkpoint。Acceptance gate逐项核对RFC owner/target闭包、closed
tracking issues、已提取的Boot Protocol baseline、Stage 1 Ready definition与resolved manifest；未发现
新的Apollyon、Keter或Euclid design finding。R0被接受，transaction建立，Stage 1进入Active，但
activation point只开放Checkpoint 1A。

Stage 1 authoritative plan和manifest只位于RFC `implementation.md`，本事务不复制第二份write-set
authority。实现反馈若改变target invariant、owner、ABI、visible semantics、acceptance boundary或
需要越出frozen manifest，立即按停止合同上报；不得通过兼容桥或双重truth绕行。

**Validation:** acceptance/activation write-back运行`git diff --check`、相对链接/生命周期残留审计与
`mdbook build docs`。没有运行xtask tests、kernel build、QEMU、rootfs、physical board或LTP；这些
不是docs-only gate的完成证据。

## Checkpoint 1A execution log

**Status:** Closed

**Change:** 新增严格`SystemTargetRef`/`PlatformRef` slug与规范化workspace-relative
`KernelConfigRef`，新增closed SystemTarget schema、五个tracked dormant target manifest和可注入
workspace root的owned loader。Canonical KernelConfig值只包含features/parameters，不携带legacy
`[build]` selection；Platform filename identity与legacy`build.name`不一致时fail fast。

**Dormant boundary:** production build、conf、QEMU、main与两份pretest wrapper均不引用新loader或
SystemTarget。Platform legacy root仍由`gen_platform_defs()`消费，是1B原子cutover前唯一behavior
source；dormant target重复值不能驱动行为。没有创建`ResolvedSystemBuild` consumer、CLI、preset、
QEMU bind、output/publication graph或host-tool abstraction。

**Review:** 独立只读review逐项核对schema、identity、owned KernelConfig、path containment、五target
matrix、dormancy与write subset；最终实现无Apollyon、Keter或Euclid finding。Residual Safe是未建立
显式symlink-escape fixture，但loader已对workspace与candidate执行canonicalize并以
`starts_with(workspace_root)`直接拒绝canonical escape，本checkpoint要求的path-normalization覆盖满足。

**Validation:** `just xtask-test`在最终字节上运行20项测试，20 passed / 0 failed，覆盖严格slug、path
规范化、missing target/platform/KernelConfig、directory拒绝、Platform filename/name mismatch、
unsupported initial-program tag、owner-external字段拒绝、五target完整load/root matrix，以及legacy
selection变化不进入owned KernelConfig。`git diff --check`与全部新文件no-index whitespace检查通过；
source audit确认production零consumer。`just fmt xtask --check`因现有fmt task把standalone xtask误当根
workspace package而报`package xtask is not a member of the workspace`，未形成格式验证；本checkpoint
不越界修复fmt owner。Kernel build、QEMU、rootfs、physical board、LTP与runtime均Not Run，不能作为
1A证据。

**Result:** Checkpoint 1A Closed。没有命中identity、owner、ABI、visible semantics、shared contract或
write-set停止条件；`BOOT-PROTOCOL-001`保持effective baseline。Checkpoint 1B仍为Not Started，不由
本closure自动进入。
