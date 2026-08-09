# RFC-20260801-exception-userptr-access

**状态：** Implemented / Closed
**修订：** R0
**负责人：** EDGW, Codex
**最后更新：** 2026-08-01
**领域：** syscall / user memory / exception / MM / RISC-V64 / LoongArch64
**事务日志：** [2026-08-01-exception-userptr-access](../../devlog/transactions/2026-08-01-exception-userptr-access.md)；
实现与验收早于本 RFC promotion，历史代码由提交 `4ed73b76`、`70f422a0` 和 `97e02808` 保存。
**影响契约：** None；R0 规则保持 RFC-local，尚未提取跨 RFC 的 user-access current contract。
**开放问题：** None；历史
[UACCESS-KETER-001](./tracking-issues.md#uaccess-keter-001---remote-fence-仍在-userspace-mutex-内完成)
已由 User TLB Completion RFC neutralize，runtime userptr默认runner边界仍在tracking页作为Safe项记录。
**下一步：** R0 无待实现阶段。若引入通用 exception table、改变 retry/partial 语义或
扩大到长期 pin/zero-copy，应启动 follow-up revision 或独立 RFC。

## 摘要

R0 用实际的、可恢复的用户内存访问取代“先遍历 VMA/PTE，再通过 Rust raw pointer 直接解引用”的旧
validator。RISC-V64 和 LoongArch64 各自提供一组 bytewise copy 汇编、精确 fault instruction label 和
fixup label；copy 期间，当前 CPU 发布一个短生命周期 `UserPtrValidation`，内核同步异常入口只有在 fault PC
精确命中该访问窗口时才记录 fault 并改写返回 PC。普通 kernel fault 仍走原 panic/page-fault 路径，不能被
userptr recovery 吞掉。

一次 copy 按页边界分块。access fault 立即转换为 `EFAULT`；可解析的 page fault 在退出异常窗口后交给
`UserSpace` resolver，成功后只重试当前剩余 chunk 一次。底层 `UserPtrAccessError::copied` 记录fault前
已经对目标可见的前缀；typed façade在failure时将其归一化为`EFAULT`，VFS则通过自己的page-bounded调用和
cursor只提交完整成功chunk，从而实现跨页“已有进展优先返回短成功”。该能力已经由用户验收；本 RFC
回顾性固化实现边界，不重演迁移。

## 背景

### 旧 validator

R0 之前的 `UserReadPtr` / `UserWritePtr` 在构造时遍历整个用户区间，对每页调用
`UserSpace::inject_page_fault()`，随后把地址转换成 Rust raw pointer、slice 或 reference 直接读写。这一形状
存在四个长期问题：

- 预检查和真实硬件访问是两套机制；真实 load/store 若仍异常，会进入普通 kernel fault，而不是返回
  `EFAULT`；
- typed pointer 强制 `align_of::<T>()`，无法兼容 Linux syscall ABI 中合法的 unaligned packed record；
- raw slice/reference helper把 user address capability暴露给调用者，难以审计 fault、partial progress和
  cleanup；
- whole-range eager fault-in不适合普通 read/write partial语义，也会让从未实际访问的后缀页承担分配成本。

旧代码从 2026-05-11 起已经保留 `TODO: tlb shootdown, out of mutex lock.`。这说明
`RemoteUspFenceGuard` 在 `UserSpace` mutex 内析构的问题早于 exception-based validator；R0 在 page-fault
resolve 和显式 `fault_in()` 路径继承了该依赖，但不是它的起源。

### 实现落点

R0 由三个历史提交组成：

- `4ed73b76`（`feat: exception-based user-pointer checker`）建立 architecture HAL、RV64/LA64 recovery
  window、kernel trap dispatch 和 typed pointer迁移；
- `70f422a0`（`feat: full support`）收口 read/getdents/clone/exit/futex 等需要 partial、pre-fault 或
  non-active address-space 特判的路径；
- `97e02808`（`feat: userptr test`）增加 `anemone-apps/userptr` 行为用例。

当前 `anemone-apps/user-test` 中的 userptr 启动项仍是手工开关注释，不是默认自动测试入口。用户已明确确认
该实现早已验收；R0 closure 记录这一验收事实，不虚构未保存的原始日志、case 计数或架构运行矩阵。

## 目标

- 让普通 copyin/copyout 的有效性由真实硬件 load/store 决定，不依赖一次静态 VMA 预检查替代访问。
- 只恢复由 accessor 自己发布、PC 精确匹配的 kernel fault；其它 kernel exception 继续 fail closed。
- 在 range overflow、kernel-space address、unmapped、permission 和 access fault 上稳定返回 `EFAULT`。
- 允许 typed syscall ABI 从 unaligned user address 拷贝，不在 kernel 中执行 unaligned typed load/store。
- 以页大小为最大 IRQ-off copy window，避免长用户 buffer 形成无界 interrupt latency。
- page fault 只在 recovery window 已撤销、硬件中断状态已恢复后解析；成功后只重试一次。
- 准确保留 fault 前已经 copy 的字节数，并由调用层决定 exact failure 或 partial success。
- 保持 `UserSpace` mutex 是 mapping topology 的 owner；copy helper和backend不得制造第二套 PTE/VMA truth。
- 为尚未 active 的 child address space 保留显式 MM probe，不伪装成可执行硬件 accessor。
- 保留窄 `fault_in()` 能力，供“必须在外部副作用前验证完整输出区间”的 transaction 使用。
- 在 RISC-V64 和 LoongArch64 上使用同一 generic contract，同时允许架构按 trap encoding表达差异。

## 非目标

- 不实现 Linux 全局 exception table、`copy_from_user()` 的所有汇编优化或 architecture-wide fixup framework。
- 不 pin 用户页，不保证 copy 前后用户内容不被其它 thread 并发修改，也不提供用户内存 snapshot。
- 不实现 zero-copy、长期 user-page loan、DMA mapping 或 `get_user_pages()` 等价能力。
- 不对同一 fault 无限重试；第二次 fault 无论类别都结束当前 access。
- 不允许 ordinary filesystem/backend保存 raw user pointer、`UserSpaceHandle` 或 recovery token。
- 不把 complete `fault_in()` 推广到 ordinary `readv`/`writev`，从而破坏已完成 segment 的 partial success。
- 不宣称所有 syscall 都具备外部副作用回滚；exact/pre-fault 仍由 syscall transaction owner显式选择。
- 不在 R0 中修复早于本方案的 remote TLB fence lock placement、IPI failure policy或通用 MM shootdown协议。
- 不允许 IRQ handler、spinlock-held path或任意 kernel code借 recovery window掩盖自身坏地址。

## 文档地图

RFC target：

- [目标与不变量](./invariants.md)：recovery window、fault matching、retry、progress、锁序与生命周期规则。
- [实现映射](./implementation.md)：三个历史提交、checkpoint、验证和验收边界。
- [Tracking Issues](./tracking-issues.md)：继承的 remote fence 风险及其 follow-up 边界。

相关已实现设计：

- [VFS Direct User I/O](../vfs-direct-user-io/index.md)：`UserBufferSink` / `UserBufferSource` 拥有普通文件
  I/O 的 user-buffer progress 与 partial policy。
- [VFS Direct User I/O 不变量](../vfs-direct-user-io/invariants.md)：backend不得获得 raw user memory
  capability，ordinary I/O 保持 `N > 0` progress优先。

Current contracts：None。未来第一次有第二个 RFC 复用或改变本协议时，应从本 R0、live source和验收证据
提取最小 user-access contract闭包，而不是把本 RFC 文本直接当作已存在的跨领域 current contract。

## 修订记录

| 修订 | 日期 | 状态 | 语义变化 | Review / 事务 |
| --- | --- | --- | --- | --- |
| R0 | 2026-08-01 | Closed | 回顾性接受 exception-backed bytewise copy、per-CPU exact-PC recovery、page-bounded window、一次 page-fault retry、partial progress和typed userptr迁移。 | [回顾性事务](../../devlog/transactions/2026-08-01-exception-userptr-access.md)；用户确认既有实现已验收。 |

## 方案

### 分层与 owner

generic trap HAL 通过 `TrapArchTrait::UserPtrAccessor` 选择架构实现，`UserPtrAccessorArch` 只暴露
`read()` 和 `write()`。`syscall/user_access.rs` 的 `UserReadPtr` / `UserWritePtr` 负责 typed range、内核
buffer转换和 syscall-facing errno；它们不再向调用者暴露“验证后可任意解引用”的 raw pointer callback。

架构 accessor 拥有：

- 可 fault 的精确汇编 instruction；
- recovery record、fault/fixup PC和trapframe PC rewrite；
- page/access fault分类；
- page-bound chunk、一次 resolve/retry和architecture-local assertions。

`UserSpace` 继续唯一拥有 VMA、PTE和lazy/COW fault resolution。accessor可以请求 resolver，但不能在
kernel trap capture阶段直接修改页表。

### 一次 access round

每个非空 chunk 按以下顺序执行：

```text
持有 UserSpace mutex并完成range检查
  -> 保存并关闭local hardware interrupts
  -> 发布per-CPU UserPtrValidation(fault_pc, fixup_pc, access)
  -> bytewise assembly copy
       -> 无fault：返回len
       -> 精确匹配fault：trap记录fault并把PC改为fixup
  -> 取走并清空validation slot
  -> 恢复进入window前的IRQ flags
  -> success / access-fault / page-fault分类
       -> page fault：UserSpace resolve，随后当前实现完成remote fence
       -> 只对剩余chunk重试一次
```

fault trap只保存 `PageFaultInfo`、fault class并改写PC。它不获取sleeping lock、不分配、不解析VMA，也不
返回 syscall errno。这样异常入口保持短小，所有可能睡眠的工作回到普通 task context。

### 汇编 copy 与精确 PC

RV64和LA64各自提供四个隐藏符号：read/write entry、fault label和fixup label。循环使用单字节 load/store；
只有访问user address的instruction位于fault label：

- read：user load有label，写kernel destination没有label；
- write：读kernel source没有label，user store有label。

fixup通过 `original_len - remaining_len` 返回已经完成的字节数。kernel buffer如果无效，fault PC不会匹配
user label，仍按kernel bug处理。使用byte instruction也意味着user address不需要满足`T`的自然对齐。

### Recovery window

每CPU只有一个 `MonoFlow<Option<UserPtrValidation>>`。`UserAccessWindow::new()` 先关闭local interrupts，
再断言slot为空并发布record；`finish()`先`take()` record，再drop window恢复IRQ。`Drop`也会在panic或早退
路径撤销slot，因此不能出现IRQ已经恢复而旧fixup capability仍可被新控制流消费的窗口。

嵌套userptr access直接assert。kernel hardware interrupt入口也assert slot为空；在当前模型中这既是
“IRQ确实保持关闭”的证明，也是future interrupt-entry改动的回归哨兵。copy loop本身不schedule、不调用
Rust callback，per-CPU record不会跨CPU迁移。

### Fault 分类与重试

accessor先按当前地址到页尾分块，因此一个window最多复制一页：

- 完整成功：推进到下一个chunk；
- access fault：保留已完成前缀并返回`BadAddress`；
- page fault：在window撤销后调用`UserSpace::handle_page_fault()`；resolver失败映射为`BadAddress`；
- resolve成功：从fault后的剩余地址重试一次；retry再fault时不进行第二轮resolve。

一次重试限制避免坏PTE、权限竞态或resolver缺陷形成kernel livelock。它不是“页面以后永远有效”的保证；
只是当前持锁mapping topology下完成一次受控恢复。

### 架构差异

| 项目 | RISC-V64 | LoongArch64 |
| --- | --- | --- |
| Trap PC | `sepc` | `ERA` |
| Fault address | `stval` | `BADV` |
| Page fault | load/store page fault | PIL/PIS/PME/PNR/PPI family |
| Access fault | load/store access fault | address error / memory access |
| Recovery match | exact PC + read/write type | exact PC；read/write type取自已发布record |
| Fixup | rewrite `sepc` | rewrite `ERA` |

LA64的异常编码不能在所有被接管路径上像RV64一样直接给出统一read/write分类，所以只用exact PC决定是否
属于当前window，再用record中的`access`构造`PageFaultInfo`。record只在IRQ-off、non-nested窗口存在，不能
被另一个access方向替换。

### Typed、exact 与 partial 语义

`UserReadPtr<T>::read()`和`UserWritePtr<T>::write()`是exact typed operation：成功必须复制完整
`size_of::<T>()`；失败返回`EFAULT`。`MaybeUninit<T>`只承接kernel destination bytes，不直接从user address
构造Rust reference。

普通file I/O由`UserBufferSource`/`UserBufferSink`在更高层按page/iovec推进cursor。已经完成`N > 0`字节后
遇到fault，调用层返回短成功`N`；没有进展时才返回`EFAULT`。architecture error中的`copied`是底层前缀
事实，不替代VFS cursor的跨segment总进度。

`try_new()`只检查address domain、overflow和长度，不承诺所有页已resident。需要在外部副作用前证明完整
write range的调用者必须显式调用`fault_in()`；fanotify exact record、getdents transaction或clone pidfd等
路径不能依赖构造器名称猜测pre-fault语义。

### Active address-space边界

exception-backed assembly只适用于当前CPU已经active的用户页表。clone child尚未切换到其新地址空间时，
parent不能用硬件accessor探测child pointer；该路径继续在child `UserSpace` 上显式注入write fault，并在child
真正运行后用exception-backed store关闭后续unmap race。显式MM probe不是第二套ordinary userptr API。

## 接受边界

R0 acceptance表示以下能力已经由用户验收并作为当前实现保留：

- RV64/LA64都有exact-PC、bytewise exception recovery实现；
- expected user load/store fault不会因普通typed copy直接升级为kernel panic；
- lazy、跨页、unaligned、permission/unmap和partial progress有对应实现与测试程序；
- typed pointer调用者已经迁移到fallible read/write API；
- special side-effect和non-active address-space路径已有显式adapter。

R0 closure不表示：

- `RemoteUspFenceGuard`已经移到`UserSpace` mutex外；
- synchronous IPI failure、CPU hotplug和完整MM shootdown协议已经证明；
- userptr app已经接入每次默认自动测试；
- user bytes在copy期间不会被其它thread修改；
- 所有syscall的side-effect/EFAULT ordering已经由本RFC逐一证明；
- 性能达到Linux优化汇编或word/vector copy水平。

改变exact-PC身份、per-CPU slot owner、IRQ publication/cleanup ordering、fault分类、一次retry、partial prefix
含义或active-address-space边界必须进入R1或follow-up RFC。只调整汇编展开、helper命名或不改变上述语义的
调用点整理，可以作为窄实现维护处理。

## 备选方案

### 保留whole-range预检查和raw pointer

拒绝。它无法让真实fault可靠返回`EFAULT`，强迫ordinary I/O eager-fault全部后缀，并持续暴露raw user
memory capability。

### 建立Linux式全局exception table

延期。全局table适合大量架构汇编site和优化copy routine，但需要linker section、排序/查找、module边界和
所有kernel fault site的统一policy。R0只有四个固定fault instruction，per-CPU exact window更小、更容易
fail closed。future扩展到更多site时可以单独提案替换。

### 在kernel trap里直接resolve并重执行

拒绝。resolver会获取MM状态、分配frame并可能触发shootdown；把这些工作放入IRQ-off trap capture会扩大
锁序、栈和reentrancy风险。trap只记录事实和fixup。

### 捕获window期间任意user-range fault

拒绝。仅按fault address或PC范围匹配会掩盖copy loop、kernel buffer或相邻Rust code的真实kernel bug。
R0只接受exact instruction identity。

### 使用word/vector copy

延期。更宽访问需要处理首尾对齐、跨页instruction、architecture fault progress和不同宽度fixup矩阵。
bytewise方案速度较低，但progress和unaligned语义直接可证。

### 无界page-fault retry

拒绝。并发mapping变化或resolver错误会形成内核livelock；第二次fault必须结束当前access。

## 风险

- bytewise loop比按machine word或vector copy慢；R0优先可恢复性和明确progress。
- 每个window最多关闭local interrupts一页copy时间；该上界仍需在future性能优化中保持可审计。
- per-CPU `MonoFlow`依赖window内不schedule/不迁移；任何preempt模型变化都必须重新证明该条件。
- user-space内容不受mutex保护；mapping topology稳定不等于bytes snapshot稳定。
- 当前remote fence在`UserSpace` mutex内同步完成，继承潜在锁序/延迟风险，见tracking issue。
- runtime userptr app不是默认runner的一部分；用户验收不能替代future持续回归自动化。

## 收口

R0实现由`4ed73b76`、`70f422a0`和`97e02808`组成，用户已确认验收通过，因此本RFC以
Implemented/Closed发布。源内两套architecture exact-match KUnit和`anemone-apps/userptr`保留回归入口。
remote fence问题作为早于R0、未被本验收消除的依赖风险继续记录；处理它时不得把“简单延迟到整次copy
结束”当作等价修复，因为COW/remap需要在当前CPU retry前完成必要的remote invalidation。
