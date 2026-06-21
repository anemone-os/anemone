# ANE-CHG-20260617-splice-copy-stage1

**Type:** Small Feature / LTP Compatibility
**Status:** Completed
**Date:** 2026-06-17
**Authors:** doruche, Codex
**Area:** VFS / pipe / syscall ABI / LTP splice

## Problem

LTP `splice07` 和 `splice03` 覆盖 `splice(2)` 的高价值 errno 矩阵。此前内核没有
`splice` / `tee` / `vmsplice` syscall 入口，也没有对应 generic syscall number，用户态调用会
直接落到 `ENOSYS`。其中 `splice07` 对无效 fd 类型组合只接受 `EBADF` 或 `EINVAL`，因此缺入口会让
这一组用例无法进入真实语义判断。

完整 Linux splice family 需要 pipe buffer 引用、页 pin、pipe buffer duplication、socket splice
和 per-call nonblocking 等机制。本轮目标是局部兼容和得分路径，不把这组能力升级为完整 zero-copy
管线。

## Scope

本轮覆盖：

- 注册 `SYS_VMSPLICE = 75`、`SYS_SPLICE = 76`、`SYS_TEE = 77`。
- 增加 `SPLICE_F_MOVE`、`SPLICE_F_NONBLOCK`、`SPLICE_F_MORE`、`SPLICE_F_GIFT` ABI 常量。
- 实现 copy-backed `splice(2)`：
  - file -> pipe；
  - pipe -> file；
  - pipe -> pipe；
  - file offset pointer 与 current file cursor 的提交；
  - `O_APPEND`、pipe offset pointer、bad fd、access mode 和 neither-pipe errno。
- 注册 error-only `tee(2)` 与 `vmsplice(2)`，覆盖 `tee02` / `vmsplice02` 的直接错误语义。
- pipe owner 只暴露 endpoint side 分类和 owner-side same-pipe 判断。

本轮不覆盖：

- zero-copy pipe buffer 引用、页 pin 或 page gift；
- `tee` 的真实不消费 input pipe 的 buffer duplication；
- `vmsplice` 的用户 iovec 搬运或 full-pipe/nonblocking 语义；
- socket splice、`/dev/zero` / `/dev/full` splice、procfs 特殊 target；
- 动态 pipe capacity、pipe resource accounting 和 `/proc/sys/fs/pipe-*`；
- `SPLICE_F_NONBLOCK` 的 per-call nonblocking I/O context。

## Solution

`splice` syscall adapter 负责 Linux ABI parse、fd lookup、access mode、offset pointer、
errno 和 copy loop。实际读写仍通过 `FileDesc::{read, write, read_at, write_at, seek}`，让
pipe 的阻塞、`SIGPIPE`、`O_NONBLOCK` 和 poll wake 继续由 pipe owner 管理。

pipe 模块新增窄接口：`PipeEndpointInfo` 只暴露 `Read` / `Write` side；
`pipe_endpoints_same_pipe()` 只返回一次性 boolean，不把 pipe object、ring buffer、poll queue
或可缓存 pipe id 暴露给 syscall 层。这样 syscall 能做 `tee` / `splice` errno 判断，但不能反向驱动
pipe 状态机。

copy-backed `splice` 使用页大小内核 buffer。非 pipe input 在 `off_in == NULL` 时先用
`read_at(current_cursor)` 读取，只有成功写出的字节会提交回 input cursor，避免 output partial
write 后把 input cursor 推过未写出的数据。offset pointer copyout 和 cursor update 失败按 state
update error 暴露；read/write error 在已经写出字节后返回 partial byte count。

`tee` 不做 consume-and-copy fallback：valid pipe pair 直接返回 `EOPNOTSUPP`。`vmsplice` 不读取用户
iovec，不 copy 或 pin 用户页：valid pipe endpoint 和合法 `nr_segs` 后返回 `EOPNOTSUPP`。这些路径都打
notice，避免调用者误认为功能语义已经可用。

## Change

- `anemone-abi/src/syscall/{riscv.rs,loongarch.rs}` 增加 generic splice family syscall number。
- `anemone-abi/src/fs.rs` 增加 `IOV_MAX` 与 `SPLICE_F_*` 常量。
- `anemone-kernel/src/fs/api/splice/` 新增 `splice`、`tee`、`vmsplice` syscall family。
- `anemone-kernel/src/fs/pipe.rs` 新增 endpoint classification 与 same-pipe owner helper。
- `anemone-kernel/src/fs/api/mod.rs` 注册新的 syscall module。

## Validation

Agent 侧已运行：

- `just fmt kernel` 通过。
- `git diff --check` 通过。
- 对新增 `anemone-kernel/src/fs/api/splice/*.rs` 运行 `git diff --no-index --check -- /dev/null ...`，
  仅返回预期的 diff exit code，没有 whitespace 诊断。
- `just build` 通过；仅保留既有 `anemone-kernel/src/sync/mono.rs` unused-import warning。

本轮未运行 QEMU / user-test / LTP runtime profile，因此不声明 `splice03`、`splice07`、`tee02` 或
`vmsplice02` 已经 runtime 通过。

## Tracking Issues

### CHG-001 - pipe owner state 不能泄露给 syscall 层

**Status:** Neutralized
**Severity:** Keter

**Issue:** `splice` / `tee` 需要知道 fd 是否是 pipe endpoint，以及两个 endpoint 是否同属一个
pipe；如果 syscall 层拿到 pipe object 或可缓存 identity，会破坏 pipe owner boundary。

**Resolution:** `pipe.rs` 只暴露 side 分类和 owner-side same-pipe boolean。syscall 层没有访问
`Arc<SpinLock<Pipe>>`、ring buffer 或 poll trigger queue。

### CHG-002 - file input cursor 只能按 written bytes 提交

**Status:** Neutralized
**Severity:** Euclid

**Issue:** copy-backed `splice(file, NULL, pipe, NULL, ...)` 如果先普通 `read()` 再遇到 output
partial write，会把 input cursor 推过未写出的数据。

**Resolution:** 非 pipe input 统一用 `read_at()` 从 snapshot offset 读取，只有成功写出的字节会推进
input offset，最后再提交 cursor 或 offset pointer。

### CHG-003 - `tee` functional path 后移

**Status:** Deferred
**Severity:** Euclid

**Issue:** 真实 `tee` 必须复制 pipe buffer 且不消费 input pipe。当前 pipe owner 没有 duplication
接口。

**Resolution:** valid pipe pair 返回 `EOPNOTSUPP` 并打 notice。剩余能力记录到 stage-1 current
limitation。

### CHG-004 - `vmsplice` functional path 后移

**Status:** Deferred
**Severity:** Safe

**Issue:** `vmsplice` functional path 会引入用户 iovec 搬运、page pin/gift、full-pipe 观测和 64K
pipe capacity 问题。

**Resolution:** 本轮只覆盖 bad fd、non-pipe fd 和 `nr_segs > IOV_MAX` 错误语义；valid functional
path 返回 `EOPNOTSUPP` 并打 notice。

### CHG-005 - `SPLICE_F_NONBLOCK` 后移

**Status:** Deferred
**Severity:** Safe

**Issue:** 当前 `FileIoCtx` 表达 opened-description `O_NONBLOCK`，不表达 syscall per-call
nonblocking flag。

**Resolution:** functional path 携带 `SPLICE_F_NONBLOCK` 时返回 `EINVAL` 并打 notice，不伪装成真实
per-call nonblocking。

## Risk / Follow-up

当前实现只证明源码层和 build gate 闭合。后续应定向运行 `splice03`、`splice07`、`tee02` 和
`vmsplice02`；copy-backed smoke 可用受控小长度运行 `splice01`、`splice02` 和 `splice04 -l 1024`。

完整 zero-copy splice、socket splice、真实 `tee`、functional `vmsplice`、per-call
`SPLICE_F_NONBLOCK` 和动态 pipe capacity 仍属于后续阶段。

## Links

- Biweekly devlog: [2026-06-08 至 2026-06-21](../2026-06-08_to_2026-06-21.md)
- Register / limitations: [splice family copy-backed stage-1](../../register/current-limitations.md#ane-20260617-splice-family-copy-backed-stage1)
- Related limitation: [pipe procfs knobs stage-1](../../register/current-limitations.md#ane-20260528-pipe-procfs-knobs-stage1)
