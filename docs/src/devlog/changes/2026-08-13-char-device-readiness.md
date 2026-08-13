# ANE-CHG-20260813-char-device-readiness

**Type:** Small iteration / local char-device capability extension
**Status:** Completed
**Date:** 2026-08-13
**Authors:** doruche, Codex
**Area:** char device / devfs / iomux readiness

## Problem / Context

`CharDev`只表达read、write、seek与ioctl，统一char devfs FileOps的poll固定返回
`NotYetImplemented`。这使具体设备无法拥有自己的readiness语义；`null`、`zero`、`full`和`urandom`
这些永久就绪源也无法通过poll/select/epoll被查询。另外，`full`已有1:7实现和注册，但没有发布到devfs。

## Decision

在`CharDev`增加typed poll hook；具体device拥有readiness predicate，char devfs只按inode `rdev`查找
registry并分发。默认实现允许无副作用snapshot返回空集合，但对需要注册且当前不ready的请求明确返回
`Unsupported`，不伪造订阅能力。

`null`、`zero`、`full`和`urandom`都是不会阻塞的永久源，只投影caller请求的readable/writable bits，
不建立route、wait queue或第二份状态。`/dev/full`即使write返回`ENOSPC`也报告writable：readiness只承诺操作
不会阻塞，不承诺操作成功。`full`同时按既有1:7设备号发布到devfs。

## Implementation Boundary

Target是让char-device框架表达device-owned readiness，并使四个memory char device和`/dev/full`发布闭合。
Owning subsystem不迁移：具体`CharDev`拥有predicate，char devfs拥有device-file dispatch，iomux继续拥有
snapshot/register/final-scan协议。本轮不修改poll/select/epoll consumer、TTY/console、block device、设备号模型、
opened-description生命周期，也不为未来stateful char source预建订阅抽象。

`IOMUX-POLL-001..003`与`DEVICE-NUMBER-001`是unchanged dependencies，不发生current contract delta或cutover。
若实现需要修改register协议、跨owner保存route、扩大公共VFS接口或引入stateful source lifecycle，本小迭代停止并
升级RFC。

## Change and Acceptance

- `CharDev::poll`接受`PollRequest`并返回`PollRegisterResult`；缺省行为fail closed；
- char devfs FileOps通过设备节点`rdev`定位已注册`CharDev`后分发poll；
- `null`、`zero`、`full`和`urandom`返回caller请求的readable/writable交集，`full`发布到devfs；
- owner-local KUnit从真实devfs open/file-poll路径检查四个设备的read/write readiness与interest filtering，并
  检查`full`的目录发布、1:7属性、zero-filled read和`ENOSPC` write；
- acceptance为RV64/LA64 release build、RV64 KUnit boot、格式、docs与source audit。无需新增用户态测试app：本轮
  只扩充source hook和devfs dispatch，不重新声明poll/select/epoll syscall ABI acceptance。

## Validation

- `just build --preset qemu-virt-rv64-release`与`just build --preset qemu-virt-la64-release`通过；
- RV64 release QEMU boot通过635/635 KUnit，focused
  `test_devfs_memory_char_device_io_readiness_and_attrs`通过，并正常进入init及关机路径；
- `just fmt kernel --check`、`git diff --check`与`mdbook build docs`通过；
- source audit覆盖全部`impl CharDev for`，确认char devfs不再固定返回`NotYetImplemented`，且本轮未引入
  `PollRoute`、wait state或TTY/console/block/iomux/epoll production改动；
- LA64 runtime和用户态poll/select/epoll端到端测试Not Run，不能从双架构build或RV64 KUnit外推。

## Remaining Risk / Links

- 后续stateful char source仍必须按`IOMUX-POLL-001..003`在source owner临界区闭合predicate与route publication；
  本轮永久就绪设备不能作为其订阅实现模板。
- Current contracts：[Poll Wait 与 Source Registration](../../contracts/iomux/poll-wait.md)、
  [Device Number](../../contracts/device/device-number.md)。
