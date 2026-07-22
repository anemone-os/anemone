# 2026-07-22 - System Target Model

**Status:** Active
**Owners:** doruche, Codex
**Area:** build system / configuration / platform / repository workflow
**Canonical Plan:** [RFC-20260722-system-target-model](../../rfcs/system-target-model/index.md), [目标与不变量](../../rfcs/system-target-model/invariants.md), [迁移实施计划](../../rfcs/system-target-model/implementation.md)
**Canonical Revision:** R0
**Current Phase:** Stage 1 Active / Checkpoint 1A

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

**Status:** Active

实现、review、validation与closure证据将在本节追加。Checkpoint 1B保持Not Started。
