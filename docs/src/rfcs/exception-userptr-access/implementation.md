# Exception-backed User Pointer Access 实现映射

**状态：** Completed
**最后更新：** 2026-08-01
**父 RFC：** [RFC-20260801-exception-userptr-access](./index.md)
**目标与不变量：** [目标与不变量](./invariants.md)
**当前契约：** None
**当前修订：** R0

## 实现说明

implementation先于公共RFC完成。本页把已经存在的三个提交、source shape、测试入口和用户验收映射为
回顾性checkpoint；它不重新授权、不重演代码迁移，也不把后来发现的`RemoteUspFenceGuard`问题写成已经
修复。执行事实由[回顾性事务日志](../../devlog/transactions/2026-08-01-exception-userptr-access.md)保存。

历史实现：

| 提交 | 角色 |
| --- | --- |
| `4ed73b76` | architecture HAL、RV64/LA64 exception window、kernel trap capture、generic typed userptr迁移 |
| `70f422a0` | read/getdents/clone/exit/futex等special route和fallible API补齐 |
| `97e02808` | `anemone-apps/userptr` runtime oracle |

实施遵循以下原则：

- 先建立exact instruction recovery和cleanup，再把generic typed API切换到fallible copy。
- trap capture与MM resolution分层；trap不获取sleeping lock。
- ordinary stream partial、exact transaction和non-active address-space probe分别由对应owner表达。
- RV64/LA64共享contract，不强迫两种trap encoding使用同一内部匹配字段。
- runtime验收按用户确认记录，不补造缺失日志或自动化覆盖。

## Checkpoint 路线图

| Checkpoint | 状态 | 交付 |
| --- | --- | --- |
| A | Closed | generic HAL、access error和两架构bytewise assembly |
| B | Closed | per-CPU recovery window与kernel trap dispatch |
| C | Closed | typed userptr façade和ordinarycopy fault/retry迁移 |
| D | Closed | special callsite、partial/pre-fault和non-active address-space closure |
| E | Closed | KUnit、runtime app和用户验收 |

## Checkpoint A - Architecture copy core

**Change：**

- `TrapArchTrait`增加`UserPtrAccessor`关联类型；`UserPtrAccessorArch`定义fallible `read()`/`write()`。
- `UserPtrAccessError`同时携带`SysError`和已经完成的连续prefix长度。
- RV64新增`__rv64_{read,write}_user_once`、fault label和fixup label。
- LA64新增对称`__la64_{read,write}_user_once`、fault label和fixup label。
- 两架构均使用byte load/store，只有user-side instruction拥有可恢复label。
- outer loop按当前address到page end切分chunk，success必须完成完整chunk。

**Proof：** 汇编remaining counter只在一次user byte access成功后递减；fixup以`len - remaining`导出
progress。read的kernel destination store和write的kernel source load不共享fault label，不能被映射为
用户`EFAULT`。

## Checkpoint B - Recovery publication与trap handoff

**Change：**

- 每CPU增加`MonoFlow<Option<UserPtrValidation>>`。
- `UserAccessWindow`关闭local IRQ后发布`fault_pc/fixup_pc/access`，normal/drop cleanup均先撤销slot。
- RV64 kernel exception入口在普通panic/page-fault dispatch前尝试capture load/store page/access fault。
- LA64 kernel exception入口在普通panic前尝试capture PIL/PIS/PME/PNR/PPI与memory access address error。
- matching trap只保存fault、rewrite `sepc`/`ERA`并返回；non-match继续原kernel exception路径。
- 两架构hardware interrupt入口断言recovery slot为空。

**Proof：**

- `validation_matches_only_exact_user_access_instruction` KUnit在两架构验证exact PC；
- RV64同一KUnit额外拒绝错误read/write type；
- `assert!(slot.is_none())`拒绝nested access；
- `assert!(validation.fault.is_none())`拒绝一个window捕获第二个nested fault；
- slot cleanup发生在`IntrGuard`恢复IRQ flags之前。

## Checkpoint C - Generic typed API cutover

**Change：**

- `UserReadPtr::read()`、`UserWritePtr::write()`、slice copy和字符串copy全部返回`Result`。
- `MaybeUninit<T>`承接typed kernel destination，不从user address构造typed Rust reference。
- constructor只检查user address domain、checked range和element-count byte overflow。
- 移除旧`with_ptr()` / `with_readable_ptr()`等raw callback以及强制typed natural alignment。
- ordinarycopy fault先分类：access fault直接失败，page fault调用`UserSpace::handle_page_fault()`后重试一次。
- MM的`InvalidArgument`、`PermissionDenied`、`NotMapped`和`RangeNotMapped`在syscall边界归一化为
  `BadAddress/EFAULT`。
- C string和pointer-array parser通过repeated fallible typed read获取内容，不再裸解引用。

**Proof：** generic façade不再公开raw user memory callback；architecture accessor成功路径assert full copy；
typed operation只在完整copy后`assume_init()`。

## Checkpoint D - Special route closure

`70f422a0`补齐了不能只做机械`Result`签名迁移的调用点：

### Ordinary read/write与partial progress

- `fs/uio.rs`的`UserBufferSink`/`UserBufferSource`按page/iovec推进cursor。
- fault前已有跨segment progress时返回短成功；没有progress时保留`EFAULT`。
- file-visible committed bytes与仅从user copy到backend临时buffer的consumed bytes继续分离。
- fallback read/write通过kernel buffer调用falliblecopy，不允许expected bad pointer成为kernel fault。

### Complete pre-fault与external side effect

- `UserWritePtr::fault_in()`/slice `fault_in()`保留explicit MM walk，只供complete-range transaction。
- getdents在推进directory backend前pre-fault完整destination，再通过kernel buffer执行最终copyout。
- fanotify exact record和其它transaction-specific caller可以在同一`UserSpace`锁域完成pre-fault + exact copy。
- ordinary vectored I/O不使用whole-vector exact transaction替代partial semantics。

### Clone、exit与robust list

- clone child尚未active时，parent使用child `UserSpace::inject_page_fault()`完成pre-publication probe；child真正
  运行后的`CHILD_SETTID` store仍走exception-backed API。
- `CHILD_SETTID`在publish后的unmap race失败时记录并跳过store，不能再向parent伪造clone失败。
- exit clear-child-tid和robust-list traversal把access failure作为fallible cleanup outcome，避免exit path因
  用户坏链表/坏地址panic。
- clone3 pidfd等必须在外部publication前成立的输出继续使用explicit `fault_in()`。

### IRQ/lock-domain callsites

- getdents不再让filesystem `read_dir` callback直接写user memory；backend先写kernel `Vec`，释放其内部lock
  domain后再copyout。
- software unaligned emulation复用typed userptr copy，因此user instruction fetch和emulated data access同样
  受exact fault recovery保护；该复用不把soft-unaligned模拟器自身的正确性纳入本RFC。

## Checkpoint E - Validation与验收

### Source/KUnit evidence

- RV64 `validation_matches_only_exact_user_access_instruction`：accept exact PC + correct access，reject
  adjacent PC和wrong access。
- LA64同名KUnit：accept exact ERA，reject adjacent ERA；access direction由active record提供。
- source assertions覆盖nested window、nested fault、hwirq entry和full-success progress。
- `UserAccessWindow::Drop`覆盖panic/early-exit slot withdrawal。

### Runtime oracle

`anemone-apps/userptr/src/main.rs`提供五组行为用例：

1. `scalar-bad-addresses`：多种read/write syscall在low/high bad pointer返回`EFAULT`；
2. `lazy-and-cross-page`：lazy copyout、跨页read copyout和write copyin；
3. `partial-cross-page-fault`：第二页`PROT_NONE`时stream I/O返回第一页prefix，exact struct返回`EFAULT`；
4. `permissions-and-unmap`：`PROT_NONE`、read-only copyout和unmapped stale address；
5. `side-effects-after-efault`：bad read不消费pipe数据、bad getdents不推进directory cursor。

该app由`97e02808`加入；`anemone-apps/user-test/src/main.rs`中的启动三行当前仍被注释，因此它是手工oracle，
不是默认持续回归。用户已明确确认R0早已验收通过。agent没有该次原始运行日志，所以本RFC不声明未提供的
架构、case count、耗时或console输出。

### Closure

用户验收、core source shape、architecture KUnit和runtime oracle共同形成R0 closure。R0没有current
contract cutover；未来出现第二个跨RFC consumer时才提取共享contract。

## 实际改动范围

核心protocol文件：

```text
anemone-kernel/src/exception/trap/hal.rs
anemone-kernel/src/arch/riscv64/exception/trap/user_ptr.rs
anemone-kernel/src/arch/riscv64/exception/trap/ktrap.rs
anemone-kernel/src/arch/riscv64/exception/trap/mod.rs
anemone-kernel/src/arch/loongarch64/exception/trap/user_ptr.rs
anemone-kernel/src/arch/loongarch64/exception/trap/ktrap.rs
anemone-kernel/src/arch/loongarch64/exception/trap/mod.rs
anemone-kernel/src/arch/mod.rs
anemone-kernel/src/syscall/user_access.rs
```

special integration集中在：

```text
anemone-kernel/src/fs/uio.rs
anemone-kernel/src/fs/api/read_write/request/{mod,read,write}.rs
anemone-kernel/src/fs/api/getdents64.rs
anemone-kernel/src/task/api/clone/{mod,clone3}.rs
anemone-kernel/src/task/api/exit/mod.rs
anemone-kernel/src/task/api/futex/{mod,futex,get_robust_list}.rs
anemone-kernel/src/arch/loongarch64/exception/unaligned/{mod,access}.rs
```

其余syscall/device/signal/time/credentials调用点主要是机械迁移到fallible read/write，完整历史集合以以下命令
从repository history恢复：

```sh
git show --name-only --format= 4ed73b76 70f422a0 97e02808
```

测试范围：

```text
anemone-apps/userptr/
anemone-apps/user-test/src/main.rs
```

## 旁路审计

后续修改本协议时至少执行：

```sh
rg -n "UserPtrAccessorArch|USER_PTR_VALIDATION|UserAccessWindow" anemone-kernel/src
rg -n "\.handle_page_fault\(|\.inject_page_fault\(|drop\(fence\)" anemone-kernel/src
rg -n "User(Read|Write)(Ptr|Slice).*try_new|fault_in\(" anemone-kernel/src
rg -n "as_ptr\(\)|as_ptr_mut\(\)|from_raw_parts" anemone-kernel/src/syscall anemone-kernel/src/fs
rg -n "assert_hwirq_not_armed" anemone-kernel/src/arch
```

分类要求：

- raw pointer只允许出现在architecture assembly bridge和kernel-owned buffer转换；
- new user-memory fault site必须有exact label/fixup/progress proof；
- explicit `fault_in()`必须说明external side-effect或exact transaction理由；
- non-active address-space `inject_page_fault()`必须说明为什么hardware accessor不可用；
- fence drop必须分类其所在mutex/IRQ context。

## 实现期反馈

### 2026-08-01 - Remote fence lock placement

只读历史与source audit确认：`RemoteUspFenceGuard`及`out of mutex lock` TODO来自`84227f55`
（2026-05-11），旧validator当时已经在持有`UserSpace` mutex时让guard析构。`4ed73b76`没有创造该问题，
但在exception page-fault retry路径保留了同样的锁内`drop(fence)`。

**分类：** inherited dependency / follow-up route；不改变R0 exception identity、copy、retry或partial target，按
用户决定不在本RFC实现修复。

**受保护边界：** future fix必须形成“锁内PTE更新和local invalidation -> 锁外必要的同步remote shootdown ->
重新加锁后才retry”的可证明handoff。不能只把guard延迟到整次copy结束，也不能用async IPI让当前CPU先retry。

**后续结果：** [UACCESS-KETER-001](./tracking-issues.md#uaccess-keter-001---remote-fence-仍在-userspace-mutex-内完成)
已由独立的User TLB Completion RFC在2026-08-09 neutralize；本段保留R0实现期反馈历史。

## Remaining Boundaries

- remote fence锁边界和IPI failure policy未由R0修复，后由
  [`MM-TLB-REMOTE-001`](../../contracts/mm/user-fault-local-tlb.md#mm-tlb-remote-001--destructive-user-mapping在dependent-continuation与retirement前完成remote-ack)
  cutover关闭。
- userptr runtime app未默认自动执行。
- bytewise copy没有word/vector性能优化。
- R0不提供page pin、content snapshot或zero-copy。
- side-effect/EFAULT ordering仍由各syscall transaction owner负责，不能从typed pointer API自动推导。
- software unaligned access模拟器的独立内存破坏问题不属于本RFC验收结论。

## 停止条件

future修改若出现以下任一情况，不能作为R0内部helper cleanup直接合入：

- exact-PC恢复改为范围/地址匹配；
- recovery state从per-CPU迁到task/global，或允许nested/迁移；
- IRQ cleanup顺序、page chunk上界、retry次数或partial prefix含义改变；
- MM resolver移入trap capture路径；
- backend获得raw user memory capability；
- active/non-active address-space边界改变；
- remote fence修复需要新的MM owner/public guard API或改变COW可见性。

这些变化应进入R1/follow-up RFC，并重新定义验证矩阵和contract impact。
