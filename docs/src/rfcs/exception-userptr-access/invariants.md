# Exception-backed User Pointer Access 目标与不变量

**状态：** Accepted Target
**最后更新：** 2026-08-01
**父 RFC：** [RFC-20260801-exception-userptr-access](./index.md)
**适用修订：** R0

本文定义exception-backed userptr accessor的correctness invariants、capability boundary和RFC-local proof
obligations。R0已经实现并由用户验收；当前没有独立的user-access current contract，未来跨RFC复用时再
提取最小contract闭包。

## 规则分类

- **Correctness Invariant：** raw user access owner、recovery identity、IRQ publication/cleanup、fault
  classification、progress、mapping topology和active address-space边界。违反会吞掉kernel bug、产生错误
  copy结果、泄漏recovery capability或形成不可控fault loop，不能作为性能折衷接受。
- **Target Guarantee / Capability：** RV64/LA64 bytewise access、page-bounded window、一次page-fault retry、
  typed exact-copy和stream partial-copy能力。改变这些能力需要target renegotiation。
- **Implementation Preference：** 汇编label名称、loop register分配、helper文件布局和内部error type名称。
  只要proof obligations不变，可以在R0内调整。

## Contract Impact

| Contract ID | 变化 | 当前规则 | Target 摘要 | 生效 Gate |
| --- | --- | --- | --- | --- |
| None | None | 当前没有跨RFC的user-access current contract | R0规则保持RFC-local | N/A |

本RFC依赖已实现的VFS user-buffer cursor边界，但不修改其owner：ordinary filesystem backend仍不得获得raw
user pointer或`UserSpaceHandle`，partial总进度仍由`UserBufferSink`/`UserBufferSource`拥有。

## Target Invariants

### UACCESS-OWNER-001 - Architecture accessor唯一拥有可恢复raw access

**规则：** 只有`TrapArchTrait::UserPtrAccessor`选出的architecture implementation可以执行可能fault的user
load/store、发布recovery identity并解释对应kernel trap。generic typed pointer、syscall helper和backend只能
通过fallible copy API访问user bytes。

**Owner：** architecture userptr accessor。

**依赖：** VFS user-buffer capability边界。

**违反表现：** 新raw dereference绕过fixup，expected `EFAULT`升级为kernel panic；或多个owner对同一fault使用
不同retry/progress规则。

### UACCESS-BOUNDS-001 - Range必须在任何硬件访问前完整验证

**规则：** `start + len`使用checked arithmetic；start必须属于user domain，end不得超过
`KernelLayout::USPACE_TOP_ADDR`。元素数量到byte长度的乘法也必须checked。失败时`copied=0`且不得执行任何
user load/store。

**Owner：** generic typed pointer与architecture accessor的入口检查。

**违反表现：** integer wrap把kernel address解释为user range，或先产生partial副作用再报告range error。

### UACCESS-WINDOW-001 - 每CPU最多发布一个recovery window

**规则：** 当前CPU的`USER_PTR_VALIDATION`只能在local interrupts disabled时从`None`转换为`Some`；window
结束前不得嵌套第二次userptr access、schedule或迁移。slot是protocol state，不是diagnostic hint。

**Owner：** `UserAccessWindow`及architecture-local per-CPU slot。

**违反表现：** trap消费错误access的fixup PC，或者迁移后faulting CPU与record owner分离。

### UACCESS-PUBLISH-001 - IRQ disable先于publication，withdraw先于IRQ restore

**规则：** `UserAccessWindow::new()`先保存并关闭local IRQ，再发布record；正常完成、fault、panic和早退都必须
先把slot恢复为`None`，之后才能恢复进入window前的IRQ flags。

**Owner：** `UserAccessWindow` RAII lifecycle。

**违反表现：** hardware interrupt或新task在旧record仍active时进入，错误获得修改kernel trap PC的能力。

### UACCESS-MATCH-001 - Recovery必须精确绑定fault instruction

**规则：** trap只有在fault PC等于当前record的单一fault label时才可capture。RV64还必须匹配trap给出的
read/write type；LA64用record中的access type构造fault info。instruction fetch fault、相邻copy instruction、
kernel buffer fault和任意其它PC不得被接管。

**Owner：** architecture `dispatch_exception()` / `try_capture_fault()`。

**违反表现：** user address触发窗口掩盖无关kernel bug，或者read window错误恢复store fault。

### UACCESS-FIXUP-001 - Trap capture只记录事实并改写PC

**规则：** 匹配的同步exception只写入一份`PageFaultInfo + CapturedFaultClass`，断言此前没有nested fault，并把
trapframe PC改为固定fixup label。capture阶段不得获取sleeping lock、分配、解析VMA、修改PTE、发送IPI或
直接生成syscall返回值。

**Owner：** kernel synchronous trap adapter。

**违反表现：** IRQ-off trap路径形成不可审计锁序/reentrancy，或fault处理与copy progress形成双重owner。

### UACCESS-ASSEMBLY-001 - 只有user-side byte instruction可恢复

**规则：** read loop只给user load设置fault label，write loop只给user store设置fault label；kernel buffer
load/store不能共享该label。copy使用byte instruction，fixup返回`original_len - remaining`。

**Owner：** RV64/LA64 userptr assembly。

**违反表现：** kernel buffer corruption被伪装为`EFAULT`；unaligned typed input触发architecture alignment trap；
或reported progress不等于已经完成的stores。

### UACCESS-CHUNK-001 - 单个IRQ-off window不得跨越用户页边界

**规则：** 每次`read_once()`/`write_once()`的长度不超过当前address到页尾的字节数，最大为一页。长buffer
必须在多个独立window之间推进，并允许IRQ flags在window之间恢复。

**Owner：** architecture accessor的outer loop。

**违反表现：** IRQ-off latency随syscall长度无界增长；一个instruction/window跨页后难以区分已解析页和下一页
fault progress。

### UACCESS-FAULT-001 - Page fault与access fault使用不同恢复策略

**规则：** access fault立即结束当前operation；page fault可以在window撤销后交给`UserSpace` resolver。resolver
失败映射为`BadAddress`，成功后只重试当前剩余chunk一次。retry再次fault时不得进行第二次resolve。

**Owner：** architecture accessor fault/retry loop。

**违反表现：** permission/access error被错误映射成lazy allocation，或mapping竞态使kernel无限重试。

### UACCESS-PROGRESS-001 - `copied`只表示已经对destination可见的连续前缀

**规则：** success必须等于请求长度；failure的`copied`必须小于当前未完成长度，并精确覆盖从operation起点
开始已经完成的连续prefix。不得把仅验证、仅读取metadata或尚未store的bytes计入progress。

**Owner：** assembly remaining counter、architecture outer loop和`UserPtrAccessError`。

**违反表现：** VFS返回未真正写入user/file的字节，或把已经可见的prefix丢成全量`EFAULT`。

### UACCESS-ABI-001 - Exact与partial policy由调用层显式选择

**规则：** typed scalar/struct/record access是exact：任一fault返回`EFAULT`。ordinary stream/vector I/O由
VFS cursor聚合progress：`N > 0`后续fault返回短成功，`N == 0`才返回`EFAULT`。architecture accessor只报告
事实，不决定syscall的外部partial policy。

**Owner：** typed userptr façade与VFS user-buffer cursor各自的调用契约。

**违反表现：** half struct被当作成功发布，或ordinary `readv`第二段fault抹掉第一段已经可见的结果。

### UACCESS-PREFAULT-001 - Complete pre-fault必须是显式transaction能力

**规则：** `try_new()`只建立typed range，不暗示所有页resident。只有在外部副作用要求完整目标区间先成立
时，transaction owner才调用`fault_in()`；ordinary vectored I/O不得借此whole-range prevalidation。

**Owner：** syscall/VFS transaction owner。

**违反表现：** 调用者因构造器名称误判mapping状态，在copy失败前消费event/fd/cursor；或ordinary partial语义
被eager validation改变。

### UACCESS-MAPPING-001 - `UserSpace` mutex唯一稳定mapping topology

**规则：** ordinaryaccess、page-fault resolution和retry期间由同一个`UserSpace`锁域保护VMA/PTE topology。
user bytes本身不受该mutex保护，不能把mapping稳定误写成content snapshot。window内不得调用可能反向获取
`UserSpace`的backend callback。

**Owner：** `UserSpaceHandle` / `UserSpace`。

**违反表现：** VMA在validation/retry中被并发移除却继续使用旧PTE；或形成`UserSpace -> backend -> UserSpace`
递归锁序。

**当前依赖边界：** page-fault mutation仍在该mutex内完成，但`UserSpaceGuard`在同步remote completion前释放
inner mutex，并只在ack后重新取得mapping owner进入retry；见
[`MM-TLB-REMOTE-001`](../../contracts/mm/user-fault-local-tlb.md#mm-tlb-remote-001--destructive-user-mapping在dependent-continuation与retirement前完成remote-ack)。

### UACCESS-CONTEXT-001 - Fault resolution只能发生在ordinary task context

**规则：** caller进入user copy时不得已经处于hardware IRQ、spinlock-held或不可睡眠backend临界区。assembly
window可以短暂关闭IRQ；slot撤销和IRQ flags恢复后才能进入MM resolver或同步IPI路径。hardware interrupt入口
观察到active slot必须assert，而不是继续处理。

**Owner：** userptr caller、`UserAccessWindow`和kernel hwirq entry共同完成handoff；slot owner仍唯一属于
architecture accessor。

**违反表现：** fault resolver在不可睡眠context获取mutex/分配，或IRQ handler与同步fault handler并发访问
同一`MonoFlow`。

### UACCESS-ACTIVE-001 - Hardware accessor只操作当前active address space

**规则：** exception-backed assembly只能访问当前CPU已经activate的用户页表。尚未运行的clone child、exec
construction或其它non-current address space必须使用owner-provided MM operation；不得临时切换页表冒充普通
copy，也不得把显式probe推广成第二套syscall userptr API。

**Owner：** task/MM lifecycle adapter。

**违反表现：** parent实际访问自己的同地址mapping却以为验证了child，或临时页表切换破坏current task/CPU
address-space identity。

## RFC-local Invariants

- R0只允许四个固定user-memory fault instruction形成recovery capability；新增site必须同时增加exact identity、
  architecture dispatch、progress proof和runtime/KUnit覆盖。
- `anemone-apps/userptr`是行为oracle，不是production owner；测试程序不能提供kernel protocol state。
- `RemoteUspFenceGuard` follow-up不得把remote invalidation简单延迟到整次copy之后。COW/remap场景要求在当前CPU
  retry/commit新mapping前完成必要的remote shootdown。
- userptr test的默认runner开关属于validation plumbing，不得反向改变production semantics以让测试通过。

## 状态所有权

| 状态 | 唯一owner | 生命周期 |
| --- | --- | --- |
| 当前recovery record | 当前CPU architecture accessor | 一个IRQ-off `UserAccessWindow` |
| fault/fixup PC identity | architecture assembly + record | build-time symbol到单次window publication |
| captured fault | active `UserPtrValidation` | synchronous trap capture到`finish()`消费 |
| chunk progress | assembly remaining counter / accessor local | 单次copy call |
| syscall/vector总progress | typed caller或VFS user-buffer cursor | 单次syscall/transaction |
| VMA/PTE topology | `UserSpace` under mutex | address-space lifetime |
| live user bytes | userspace及其共享mapping | 不由kernel mutex提供snapshot |
| trapframe PC | architecture kernel trap adapter | exception entry到fixup return |

## 身份与能力模型

- recovery capability由`(current CPU, exact fault PC, access direction, active window lifetime)`共同命名。
- user virtual address不是recovery identity；相同address在其它kernel instruction fault不能使用当前fixup。
- `UserPtrValidation`不是可clone token，不得保存到task、callback、event或跨CPU队列。
- `UserReadPtr`/`UserWritePtr`只是短生命周期typed copy façade，不证明页面未来resident。
- `fault_in()`是complete-range pre-fault capability，只能由需要该语义的transaction owner显式调用。
- `UserBufferSink`/`UserBufferSource`拥有跨chunk/segment cursor，不得由architecture accessor复制总progress。

## 线性化点

- recovery publication：local IRQ已经关闭后，slot从`None`写为`Some(record)`。
- fault capture：matching trap把`fault`从`None`写为`Some`并把trap PC改成fixup的同一顺序段。
- recovery withdrawal：`slot.take()`；此后同一fault不能再次借当前window恢复。
- byte progress：每次user-side store成功后、remaining counter递减；fixup据此导出连续prefix。
- page resolution：`VmArea::handle_page_fault()`完成PTE update和local TLB invalidation。
- typed exact publication：完整copy返回后，caller才可把kernel value/record视为有效。
- VFS partial publication：cursor只按copy helper实际返回的bytes推进。

remote TLB shootdown的长期线性化和failure policy不由R0重新定义；当前依赖风险见tracking issue。

## 锁序与生命周期规则

- ordinary顺序是`caller/VFS lock（若已被对应RFC接受） -> UserSpace mutex -> 短IRQ-off copy window`。
- IRQ-off window中不得获取mutex、分配、发送IPI、调用filesystem/backend或schedule。
- synchronous fault trap只访问current CPU slot和当前kernel trapframe。
- window撤销后恢复IRQ flags，随后才允许在已持有的`UserSpace`锁域内解析page fault。
- nested userptr access必须assert；不能用stacked slots或覆盖旧record“兼容”。
- cleanup先撤销published slot，再恢复IRQ；即使panic最终停止kernel，也不能留下可被后续流消费的capability。
- caller若持有会在MM fault path反向获取的锁，必须在进入userptr前重排transaction；不能把锁序责任下推给
  architecture trap handler。
- non-active address space probe不得调用assembly accessor。

## 禁止退化项

- 重新通过Rust reference/raw slice直接访问user range。
- 只按fault address、任意kernel text range或“slot非空”决定fixup。
- 允许一个window跨多个页面或整个任意长度syscall buffer。
- 在slot仍为`Some`时恢复hardware interrupts、schedule或迁移。
- 在trap capture路径调用`UserSpace::handle_page_fault()`。
- 把access fault当作lazy page fault解析。
- 第二次fault后继续无限resolve/retry。
- 把`UserPtrAccessError::copied`解释成file-visible committed bytes。
- ordinary vectored I/O使用whole-vector `fault_in()`改变partial policy。
- backend直接持有user address/recovery record并自行实现copy fault policy。
- 用async remote shootdown或copy结束后的延迟drop替代retry前需要完成的同步fence。

## 非目标

- 全局exception table、word/vector optimized copy、page pin、zero-copy和user content snapshot。
- 为所有syscall集中定义side-effect rollback语义。
- 证明旧`RemoteUspFenceGuard`的锁序、IPI allocation failure或CPU-hotplug correctness。
- 支持从IRQ/spinlock/non-current page table访问任意user pointer。

## 完成标准

- RV64和LA64各自只有fixed read/write fault label能进入userptr fixup。
- exact-match KUnit覆盖合法PC和相邻PC；RV64额外覆盖错误access type。
- range、lazy mapping、cross-page、unaligned、permission、unmap、partial和side-effect场景有user app oracle。
- typed pointer API全部改为fallible copy，不再公开旧raw callback。
- non-active clone child和完整pre-fault调用点有显式source audit。
- page chunk最大一页，slot cleanup先于IRQ restore，hwirq entry保留active-slot assertion。
- 用户确认R0运行验收通过；未保存的原始日志不得由文档补造。
- inherited remote fence问题被明确隔离，不把R0 closure扩写成该问题已修复。

上述R0范围已经实现并由用户验收，状态为Closed；没有current contract cutover。
