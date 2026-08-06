# ANE-CHG-20260804-unix-seqpacket

**Type:** Small Feature / contract-bearing local cutover
**Date:** 2026-08-04
**Authors:** doruche, Codex
**Area:** Socket front / Unix IPC / pathname namespace / iomux / Rust consumer

## Problem / Context

Rust `std::process::Command`在Linux fork/exec路径使用
`socketpair(AF_UNIX, SOCK_SEQPACKET | SOCK_CLOEXEC, 0)`传递child exec error：exec成功时child端随
`CLOEXEC`关闭并让parent读到EOF，失败时child发送一个完整errno/footer record。只实现这条8-byte偶然调用形状会把
consumer细节固化成ABI；本轮因此交付connection-oriented Unix seqpacket的自然局部能力，同时复用现有pathname
control plane和opened-description lifecycle。

现有Unix stream owner已经拥有endpoint role、listener/backlog、pathname identity/DAC、connect/accept admission、
address snapshot、final release及wait handoff。本次需要在不复制这些truth、不改变stream byte semantics、也不把
record policy上移到general Socket front的前提下，增加独立的record data plane。target、owner、failure/cleanup、
ABI、contract delta与validation在CKPT 1前已经闭合，因此工作保持为至多两个execution checkpoint的小迭代；CKPT 2
承担唯一semantic/contract cutover。

## Decision

- 支持`AF_UNIX + SOCK_SEQPACKET + protocol 0`的`socket`、unnamed `socketpair`、filesystem pathname
  `bind/listen/connect/accept/accept4`、address/option query、read/write/vector/sendto/recvfrom、creation flags、
  `MSG_DONTWAIT/MSG_NOSIGNAL/MSG_PEEK/MSG_TRUNC`、connected shutdown、poll/select/epoll及final-close lifecycle。
- endpoint的immutable profile只服务Unix owner内的pathname admission与connection construction；general Socket的
  immutable static descriptor仍是front/UAPI semantic type的唯一witness。cross-profile pathname connect在backlog、
  publication或role commit前返回`EPROTOTYPE`。
- stream和seqpacket共享connection-oriented control/lifecycle owner，但data plane分离：stream direction继续唯一拥有
  byte queue和partial progress；seqpacket record direction唯一拥有record queue、whole-record transaction、byte/count
  capacity、terminal与readiness。
- 一次成功的非空send发布一个完整record；一次receive至多观察或消费一个record。short receive消费整个record，
  `MSG_TRUNC`返回原record长度，`MSG_PEEK`不消费。copyout必须完整复制目标prefix后才消费，`EFAULT`保留head。
- 默认单record maximum、每direction byte budget、每direction record-count budget分别为65536、65536和128。oversize
  返回`EMSGSIZE`；capacity不足进入共同blocking wait或nonblocking `EAGAIN`；send terminal映射`EPIPE`及可选
  `SIGPIPE`。
- 当前不发布zero-length record，zero-length receive不观察或消费head；该选择和receive copy-fault retention相对Linux
  的差异登记为`ANE-20260804-UNIX-SEQPACKET-EDGE-ABI`。pidfd、`sendmsg/recvmsg`、`SCM_RIGHTS`及其它ancillary
  data不在本轮范围。

## Implementation Boundary

general Socket front拥有descriptor/private envelope、Linux ABI containment、fd preparation及共同
`EAGAIN -> wait -> recheck`；Unix endpoint/listener/namespace拥有role、pathname binding/admission、backlog、address
snapshot及control-plane lifecycle；stream与seqpacket direction分别唯一拥有各自data-plane state。socketpair/accept
沿用unpublished preparation与infallible publication，semantic final release先撤销endpoint publication，再由当前
listener/connection/direction owner提交drain、EOF/EPIPE与route snapshot，guard外notify/drop。

本轮不改变VFS、opened-description、iomux或epoll owner，不建立generic transport registry/vtable、第二份type/
readiness/lifecycle truth、cross-owner lock或production probe。实现需要改变这些边界、扩大contract impact、降低
acceptance或增加第二次cutover时必须停止并升级RFC。

## Change

- 增加`UnixSeqpacket` static descriptor、resolver/query/file-I/O dispatch与`EPROTOTYPE` errno映射；Socket adapter
  增加family-neutral exact-copy sink与typed receive outcome，以表达record-aware copyout而不泄漏Unix private state。
- CKPT 1的stream-only admission capability扩展为profile-compatible `ConnectionAdmission`；namespace继续以exact
  inode identity、generation和weak endpoint publication为唯一live binding truth。
- 新增`endpoint/record.rs`，以direction-local operation gate、spinlock外staging/copy和commit前association/
  terminal/capacity recheck实现whole-record send、one-record receive、bounded accounting、shutdown及readiness。
- 新增owner-local inline KUnit和同源userspace seqpacket suite；suite覆盖unnamed/pathname、flags、record boundary、
  short/peek/truncation、fault retention、byte/count pressure、cross-profile admission、shutdown/readiness与共享endpoint
  multi-writer/reader。
- 独立final review发现public `POLLOUT`只承诺一个byte和一个record slot，而blocking send曾用它等待任意payload，
  在小record释放后可能持续busy retry。共同Socket retry现在允许family提供operation-specific `SocketWait`；seqpacket
  wait source携带immutable payload length、复用既有direction route与register/recheck协议，并以exact byte/count
  admission或terminal作为predicate。public readiness的low-watermark语义保持不变；owner KUnit与userspace
  `payload-specific-blocking-send-wait`共同覆盖该区别。
- 同一review发现receive staging的infallible `Vec::with_capacity(record_len)`会把可恢复的内存压力升级为kernel
  allocation failure；现改用`try_reserve_exact`并返回`ENOMEM`，同时保留head record与capacity供调用者重试。
- 新增静态Rust-musl `rust-command-test`，以普通`std::process::Command`验证exec success的CLOEXEC EOF与forced
  exec failure的完整errno record；child re-exec同一binary，不依赖rootfs额外工具。
- pretest rootfs接入该consumer，并保留UDP、Unix stream、ICMP raw及glibc/musl socket oracle作为共同front/
  ABI/wait回归。
- RV64四HART完整KUnit尝试暴露了相邻TTY测例
  `break_flush_keeps_later_worker_batch_units_and_needs_no_isig`自身错误；按维护者指示删除该测例及两个仅供它使用的
  helper。该删除不作为seqpacket验收证据，也不触发额外build/runtime验证。

## Checkpoints

### CKPT 1 - Connection-oriented control preparation

**Purpose:** 在semantic cutover前独立收敛stream已有pathname admission handoff。

**Deliverable:** stream-only typed capability、connect wait revalidation与exact identity/generation KUnit；resolver、ABI、
stream byte direction和current contract保持不变。

**Validation:** `just fmt kernel --check`、`git diff --check`、`just test xtask` 81/81、RV64/LA64 release build及两架构
KUnit runtime通过；独立review的capability过宽Euclid已在CKPT 1内修正。userspace socket regression当时因受限环境
不能取得交互式sudo而Not Run。

**Cutover:** `None`。

**Stop / Result:** 独立安全闭包已提交为`8f5cd4c442d9bbb2cba0e89cf8112731a82d5bf3`；用户随后授权CKPT 2。

### CKPT 2 - Seqpacket target closure

**Purpose:** 在同一已解析边界内交付record owner、完整target ABI与一次最终contract cutover。

**Deliverable:** static descriptor、profile-compatible pathname admission、独立record data plane、focused tests、普通
Rust `Command` consumer、current contract及accepted limitation。

**Validation:** 双架构release/app build与真实guest suite、RV64四HART focused runtime、Linux edge-ABI
characterization、最终format/xtask/docs/diff checks及独立change review；详见下节。

**Cutover:** `SOCKET-UNIX-SEQPACKET-CUTOVER`原子Introduce `UNIX-SOCKET-SEQPACKET-001`，Refine
`UNIX-SOCKET-STATE-001`、`UNIX-SOCKET-NAMESPACE-001`与`UNIX-SOCKET-LIFECYCLE-001`。

**Stop / Result:** target、acceptance及contract impact未扩张；独立final review的payload-specific wait Apollyon与
receive allocation Euclid均在原Implementation Boundary内修复，复审确认没有剩余blocking owner、lifecycle、ABI、
admission、record transaction或readiness问题。本提交原子关闭checkpoint与contract cutover。

## Contract Impact / Cutover

| Contract ID | 变化 | Cutover前baseline | 新规则 | 生效证据 |
| --- | --- | --- | --- | --- |
| `UNIX-SOCKET-SEQPACKET-001` | Introduce | None | 独立record transaction、capacity、terminal/readiness与支持ABI | owner KUnit、双架构guest、RV64四HART、Rust consumer |
| `UNIX-SOCKET-STATE-001` | Refine | endpoint/listener/stream direction各自拥有truth | endpoint增加immutable owner-local profile，typed stream/seqpacket connection与direction仍分离拥有state | source/lock audit、KUnit与stream/seqpacket guest |
| `UNIX-SOCKET-NAMESPACE-001` | Refine | exact inode/generation解析live stream binding | live binding提供profile-compatible窄admission；cross-profile在任何connection commit前`EPROTOTYPE` | namespace/admission KUnit与pathname guest |
| `UNIX-SOCKET-LIFECYCLE-001` | Refine | publication/retirement协议覆盖stream connection/direction | 同一prepare/commit/final-release协议覆盖seqpacket connection与record direction | rollback/final-close KUnit与guest |

`SOCKET-FRONT-001`、`SOCKET-ABI-001`、`SOCKET-WAIT-001`、`UNIX-SOCKET-STREAM-001`、
`UNIX-SOCKET-ADDRESS-001`、opened-description、iomux与epoll规则均为Dependencies。live diff中的descriptor
capability、exact-copy sink和typed outcome保留既有front owner、ABI containment及wait/recheck职责，不形成这些ID的
semantic Refine。代码、current contract、register与本记录在同一个CKPT 2提交中生效；若提交未完成则旧contract保持
current。

## Validation

- Build/app：RV64与LA64 release kernel、`socket-test`、`user-test`、`rust-command-test`均通过repository entrypoint
  构建；Rust consumer的direct qemu-user运行通过。架构build与DTB使用保持串行。
- pre-review RV64 normal pretest：417/417 KUnit通过；UDP 16/16、Unix stream 23/23、seqpacket 5/5、ICMP raw 10/10、Rust
  `Command` 2/2、glibc socket LTP 3/3与musl socket LTP 3/3全部通过，最后正常PowerOff。
- pre-review LA64 normal pretest：422/422 KUnit及上述同源userspace suite全部通过；guest发起orderly PowerOff后到达已知的
  `no power off handler succeeded, halting the system`，QEMU随后由runner终止。
- 上述pre-review KUnit计数取得于维护者要求删除错误TTY测例之前；删除仅移除该测例及专用helper，按维护者明确指示未为此
  再运行build/runtime，因此不把旧计数表述为删除后的重新执行结果。
- post-review RV64 normal pretest在最终wait/allocation修复后通过417/417 KUnit；UDP 16/16、Unix stream 23/23、
  seqpacket 6/6（新增`payload-specific-blocking-send-wait`）、ICMP raw 10/10、Rust `Command` 2/2、glibc与musl
  socket LTP各3/3全部通过并正常PowerOff。该运行服务seqpacket修复验收，不为TTY删除建立额外验证义务。
- post-review LA64以`qemu-virt-la64-release`、`smp=1`、`memory=1G`完成release kernel构建；最终wait/allocation
  修复后的第二次LA64 guest runtime未运行，不能用pre-review guest log替代。
- RV64 `smp=4`：生成配置确认`MAX_LOGICAL_CPUS=4`，OpenSBI确认HART `0*,1*,2*,3*`。完整KUnit boot在上述无关
  TTY测例panic后停止；禁用KUnit的focused guest随后让UDP 16/16、stream 23/23、seqpacket 5/5（含共享endpoint
  multi-writer/reader）、ICMP raw 10/10、Rust consumer 2/2和两套LTP 3/3全部通过并正常PowerOff。验证后
  `conf/.defconfig`恢复`kunit=true`与`max_logical_cpus=1`。
- Linux 6.6.32 host characterization观察到zero-length send发布readable record、zero-length receive消费head、
  payload copy `EFAULT`消费record；Anemone的差异已收敛为单一accepted limitation。tracked source入口为
  `xref:linux-6.6.32:net/unix/af_unix.c`。
- 独立final review确认上述两个finding已经修复且没有剩余blocking finding；残余验证边界按本节明确保留，私人
  草案不作为public证据。
- 最终静态门禁通过：kernel、`socket-test`、`user-test`、`rust-command-test` format check，`just test xtask`
  81/81，`mdbook build docs`、tracked/untracked whitespace check及残留引用检查。
- **Not Run:** physical hardware、LA64 `smp>1`、其它SMP拓扑、full socket/network LTP、final harness、pidfd、
  `sendmsg/recvmsg`、`SCM_RIGHTS`及其它ancillary data。

## Remaining Risk / Links

- [Unix seqpacket当前契约](../../contracts/socket/unix-seqpacket.md)是effective record语义唯一正文。
- [`ANE-20260804-UNIX-SEQPACKET-EDGE-ABI`](../../register/current-limitations.md#ane-20260804-unix-seqpacket-edge-abi)
  保存zero-length record与receive copy-fault差异；pre-connection shutdown、retired bind inert inode和non-UTF-8
  pathname仍由既有register条目拥有。
- Linux source与host characterization只证明已列ABI事实，不把Linux内部对象或锁形状提升为Anemone contract。
