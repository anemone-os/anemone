# ANE-CHG-20260804-unix-seqpacket-ckpt1

**Type:** Small Feature / checkpointed preparation
**Date:** 2026-08-04
**Authors:** doruche, Codex
**Area:** Socket front / Unix IPC / pathname admission

## Problem / Context

Unix stream 已经拥有 pathname bind、listener admission、connect/accept、address snapshot、wait handoff 与
final-release 生命周期。下一步 seqpacket 需要复用这条 connection-oriented control plane，但不能让 namespace
lookup 把无类型 endpoint capability 直接泄漏给调用者，也不能在 CKPT 1 提前发布 seqpacket ABI、profile 或
第二份 role/type truth。本 checkpoint 因此只整理现有 stream consumer 的 admission handoff，并保持当前
stream visible semantics 与 current contract 不变。

## Decision

namespace 的 live binding 现在只向 stream admission 提供带 inode identity 与 generation 的
`StreamAdmission` capability。capability 自己拥有 listener revalidation、local-name handoff 与 stale-binding
检查；connect wait source 只保存该 capability，不再直接读取 pathname endpoint state。endpoint role、binding
publication、listener queue、route ownership 与 final release 仍由现有 Unix owner 负责，namespace registry 仍
是 exact inode/generation 的唯一 publication/withdraw authority。

该 capability 是同一 Unix owner 内的 typed handoff，不是 public API、SocketType tag 或未来 transport registry。
CKPT 2 才会增加第二种 admission profile 和 seqpacket ABI；本次不修改 resolver、Socket ABI、stream data plane
或 current contract。

## Change

- `LiveBinding::stream_admission()` 将 namespace identity 与 endpoint lifetime 一起封装为 stream-only capability。
- `StreamAdmission` 只暴露 local-name、listener-current 与 binding-generation revalidation；endpoint Arc/state
  保持 capability 内部私有。
- connect admission 与 blocking wait recheck 改用该 capability；accept/public listener poll 继续使用既有
  endpoint owner 路径。
- namespace KUnit 增加 generation match / stale identity 断言；admission KUnit fixture 通过真实 published
  registration 构造 stream capability，并在测试结束撤销 registration。

## Checkpoints

### CKPT 1 - Connection-oriented control preparation

**Purpose:** 在 seqpacket semantic cutover 前独立收敛 stream 已有 control-plane handoff，保留一个可审阅、可回滚的
behavior-preserving checkpoint。

**Deliverable:** stream-only `StreamAdmission` seam、connect wait handoff 与 owner-local identity tests。

**Validation:** 源码审计确认 resolver/ABI/SocketType、stream byte direction、listener queue、lifecycle 与
current contract 未变；RV64/LA64 release build 均启用 `kunit` 并通过；format/diff checks 与 xtask tests 通过。
两架构 KUnit runtime 已通过；socket userspace regression 因 rootfs materialization 需要交互式 sudo 而未运行，
见 `Validation`。

**Cutover:** `None`；不接受 `SOCK_SEQPACKET` tuple，不改变 stream visible semantics 或 current contract。

**Stop / Result:** 独立 review 未发现 Apollyon/Keter。review 提出的 capability 过宽 Euclid 已在本 checkpoint
内收窄为 capability-owned revalidation；CKPT 2 未授权，工作在此停止。

### CKPT 2 - Seqpacket target closure

保留在同一 Implementation Boundary 内，尚未授权或实现。该 checkpoint 才能增加 seqpacket admission profile、
record data plane、ABI 分发、focused guest validation 与唯一最终 contract cutover。

## Validation

- `just fmt kernel --check`：通过。
- `git diff --check`：通过。
- `just test xtask`：81/81 通过。
- `just build --preset qemu-virt-rv64-release --bind smp=1 --bind memory=1G`：通过；构建启用 `kunit`、`fs_ext4`、
  `spin_lock_irqsave`、`soft_unaligned_access` 与 `kernel_symbols`。
- `just build --preset qemu-virt-la64-release --bind smp=1 --bind memory=1G`：通过；同样启用 `kunit` 与 release
  features。
- RV64 QEMU 使用当前 kernel 与现有 repository acceptance rootfs：`412/412 KUnit` 通过并正常 PowerOff；覆盖
  Unix namespace/admission/endpoint 用例。
- LA64 QEMU 使用当前 kernel 与现有 repository acceptance rootfs：`417/417 KUnit` 通过；QEMU 在既有无 poweroff
  handler 的 halt 后由 runner 终止。该差异不影响 KUnit marker。
- 独立 subagent review：通过；无 Apollyon/Keter。初始 Euclid（connect wait 直接取得 endpoint Arc）已按 review
  反馈改为 capability-owned listener/binding handoff。
- `./scripts/run-user-test-rv64.sh ...`：rootfs app staging 完成，但 `virt-make-fs` 因 sandbox 外无交互 tty
  无法读取 sudo 密码而停止；因此 pathname/socketpair/shutdown/readiness userspace regression 为 **Not Run**。
- LA64 guest runtime、双架构 userspace socket regression、SMP>1、LTP、physical hardware：**Not Run**。

## Remaining Risk / Links

- CKPT 1 只证明 typed control-plane handoff 的编译与源码不变量；不能外推 seqpacket record semantics 或
  guest runtime closure。
- `ANE-20260802-UNIX-PRECONNECTION-SHUTDOWN`、`ANE-20260802-UNIX-BIND-RETIRED-INERT-INODE` 与 VFS
  non-UTF-8 pathname limitation 保持不变，本 checkpoint 不更新 register/current contract。
- 下一步必须在 CKPT 2 pre-cutover audit 中确认该 seam 是否足以承载第二种 admission profile；若需扩大 owner/API/
  contract boundary，应停止并升级 RFC。
