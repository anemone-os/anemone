# 2026-07-22 - System Target Model

**Status:** Active
**Owners:** doruche, Codex
**Area:** build system / configuration / platform / repository workflow
**Canonical Plan:** [RFC-20260722-system-target-model](../../rfcs/system-target-model/index.md), [目标与不变量](../../rfcs/system-target-model/invariants.md), [迁移实施计划](../../rfcs/system-target-model/implementation.md)
**Canonical Revision:** R0
**Current Phase:** Stage 1 Active / Checkpoint 1A Closed / Checkpoint 1B Closed / Checkpoint 1C Not Started

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

## Checkpoint 1B activation - 2026-07-23

用户原始授权明确要求完成Stage 1前两个checkpoint。1A已独立关闭并以`a22fb460`提交后，本事务
单独记录1B activation；该activation不来自1A自动推进。当前write subset、review/validation、恢复与
停止条件仍以canonical `implementation.md`为唯一权威。

Checkpoint 1B只完成single resolver snapshot、build consumer/root owner原子cutover与Stage 2必须删除
的legacy selection bridge。U-Boot post-link重构、workflow/durable-surface closure、QEMU/DT、Source
driver和Boot Protocol cutover均未获授权；1B关闭后不进入1C。

## Checkpoint 1B execution log

**Status:** Closed

**Change:** Legacy kconfig selection现在只调用一次resolver。Resolver在snapshot边界内解析selected
kconfig与必要的`.defconfig`默认值，把完整KernelConfig、SystemTarget、Platform、profile及canonical
refs固定到owned`ResolvedSystemBuild`；`gen_kconfig_defs()`不再在build consumer中重读默认配置。
Build只接收该snapshot与action-local`disasm`，并从SystemTarget root生成原有kernel常量。全部tracked
Platform已删除legacy root字段；`conf switch`与两份pretest wrapper只保留Stage 2必须删除的legacy
selection bridge。Wrapper显式区分SystemTarget与legacy QEMU Platform identity，不依赖同名巧合驱动
不同owner boundary。

**Review:** 首轮独立只读review发现三个Keter：defs生成在resolve后重读`.defconfig`、wrapper混用
target/platform identity、no-U-Boot测试没有连接实际post-link分支；另有一个RFC lifecycle Euclid残留。
修复后，resolver内物化全部参数默认值，temp-workspace mutation测试同时改写selected kconfig、`.defconfig`、
SystemTarget与Platform并证明snapshot/defs不变；wrapper拆分两种identity并写明Stage 2退出条件；实际
`build_uboot_image()`在构造任何command前先匹配被测试的`UbootPlan::Skip`。复核无Apollyon、Keter或
Euclid finding，20个modified file仍全部位于冻结1B/Stage 1 manifest。

**Validation:** 最终字节运行`just xtask-test`，26 passed / 0 failed，覆盖五target matrix、同一target
dev/release profile、invalid architecture/root source、完整默认值物化、canonical mutation后的owned
snapshot不变、root owner cutover及no-U-Boot skip分支。`just xtask conf list`、两份wrapper的`bash -n`、
`git diff --check`与source/residual audit通过；audit确认build没有`KConfig::from_str`、
`PlatformConfig::from_str`或Platform路径直读，`.defconfig`只在resolver内作为默认输入读取，tracked
Platform不再存在`[rootfs]`，production也没有`build.platform`残留。`mdbook build docs`通过，只报告
既有large search-index warning。`just fmt xtask --check`仍在执行格式检查前因现有root workspace不包含
standalone`xtask`而失败，未形成format validation；本checkpoint不越界修复fmt owner。Kernel build、
QEMU、rootfs、physical board、LTP与runtime均Not Run，不计入1B证据。

**Result:** Checkpoint 1B Closed。没有命中owner/API/shared-contract/ABI/visible-semantics/
target-invariant/write-set停止条件；Stage 1仍为Active，Checkpoint 1C保持Not Started且未获授权。
`BOOT-PROTOCOL-001`继续由effective baseline生效，本checkpoint没有contract cutover。
