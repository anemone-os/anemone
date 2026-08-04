# ANE-CHG-20260805-brk-shrink-decommit

**Type:** Bugfix
**Date:** 2026-08-05
**Authors:** EDGW_, Codex
**Area:** mm / user address space / brk / anonymous VMO

## Problem / Context

`UserSpace::set_brk()`在跨页shrink时只撤销`[PAGE_ALIGN(new_brk), PAGE_ALIGN(old_brk))`的PTE，heap
backing仍由`AnonObject.pages`持有原`FrameHandle`。后续regrow触发page fault时，`AnonObject`直接返回旧frame，
使本应按Linux匿名新页语义归零的整页重新暴露shrink前内容。glibc allocator可以依赖新增长sbrk页为零，
因此该偏差能够形成错误的`calloc`结果；它是RV64 cc1内存破坏的候选链，但尚无PPN/generation证据确认因果。

fork后不能仅删除`ShadowObject.overlay`：后续read fault会继续委托parent，使当前地址空间重新观察fork前内容。
同时，若decommit直接drop最后一个backing handle，remote CPU在TLB shootdown完成前仍可访问已经回到frame
allocator的PPN。因此内容语义和frame retirement必须在同一局部修复中闭合。

## Decision

本轮保持`UserSpace`为brk、PTE和fence编排owner，VMO为页面内容owner：

1. VMO的`decommit_range()`先提交内容语义，并把移出的`FrameHandle`作为`RetiredFrames`能力交给address-space owner；
2. `UserSpace`随后撤PTE并完成local invalidation，`RemoteUspFenceGuard`在mutex外同步执行remote shootdown；
3. 只有remote completion成功后才drop退休frame；IPI失败时沿用现有不可返回的`Drop`边界，fail-close保留frame并告警；
4. `ShadowObject`用规范化decommitted range遮蔽parent，read返回共享零页，write从zeroed frame建立新overlay；
5. 只处理完整页区间，同一页面内的brk shrink保留现有映射和内容。

这是一个同一MM owner内、单次提交即可闭合的局部语义修复：不改变syscall ABI、public errno、shared contract或
跨subsystem owner，因此使用小迭代而不是RFC。类型和helper不作为新的公共VM contract。

## Change

- `AnonObject`支持移出指定页范围并返回退休frame；普通discard继续保持原调用语义。
- `ShadowObject`把overlay和decommitted range收在同一锁内；decommit遮蔽parent，write fault原子消费单页遮蔽。
- `set_brk()`在撤PTE前decommit完整shrink页，字节级同页shrink不创建fence或改变backing。
- `RemoteUspFenceGuard`携带退休frame；成功shootdown后自然释放，失败时保留并输出高优先级诊断。
- owner-local KUnit覆盖普通heap整页shrink/regrow归零、同页保留、fence前frame持有，以及Shadow parent/overlay遮蔽。

Implementation Boundary：本轮不启用或修改`MADV_DONTNEED` syscall，不修file-backed mmap/truncate，不改变通用
user unmap transaction、IPI transport、CPU hotplug或ASID策略，也不把本修复写成cc1最终根因确认。

## Validation

- `just fmt kernel --check`：通过。
- `just build --preset qemu-virt-rv64-release --bind smp=4 --bind memory=2G`：通过。
- `just build --preset qemu-virt-la64-release --bind smp=4 --bind memory=2G`：通过。
- RV64 pretest按仓库wrapper的rootfs、测试盘和QEMU参数完成运行；宿主非交互`sudo`不可用，因此rootfs制作和
  测试盘staging在`LOCAL.md`指定的`gallant_lamarr`容器内完成，kernel build与`just qemu`在宿主完成。
  QEMU运行了471个KUnit并输出`All tests passed!`；其中
  `brk_shrink_decommits_full_pages_and_preserves_partial_page`和
  `decommit_masks_overlay_and_parent_contents`均通过。随后native userspace、socket与pipe capacity检查通过，
  guest有序关机。完整日志位于gitignored的`build/brk-shrink-rv64.log`。
- `mdbook build docs`：Not Run；用户明确要求跳过全部mdBook测试。
- `git diff --check`：通过。

## Architecture Friction

**Euclid - remote fence failure只能在Drop中fail-close泄漏退休页。** `RemoteUspFenceGuard`没有可返回、重试或隔离后
回收的completion API；若IPI allocation/offline失败，本轮只能永久保留受影响frame，避免更严重的stale-TLB物理
别名。代码注释将移除条件固定为“shootdown具有infallible或retryable owner API”。该偏差继承自现有MM fence协议，
最小后续方向是为address-space owner提供显式completion/failure handoff，而不是让VMO或frame allocator感知CPU。

## Remaining Risk / Links

- Current contract：None。
- Register / limitation：无新增条目；remote fence继承问题见
  [Exception-backed User Pointer Access tracking](../../rfcs/exception-userptr-access/tracking-issues.md#uaccess-keter-001---remote-fence-仍在-userspace-mutex-内完成)。
- `MADV_DONTNEED`仍是成功no-op，file mmap/truncate lifetime仍不在本轮处理。
- cc1数据破坏的最终根因仍需损坏slot的VPN/PTE/PPN/generation证据；本轮只关闭`brk`候选本身的代码缺陷。
- 外部源码证据：None；Linux可见语义由本轮focused oracle验证，不新增私人checkout引用。
- Issue / PR / commit：本change record所在实现commit。
