# ANE-CHG-20260802-vfs-kernel-creation-boundary

**Type:** Cleanup / Contract-bearing Small Iteration
**Status:** Completed
**Date:** 2026-08-02
**Authors:** doruche, Codex
**Area:** VFS creation / syscall-facing kernel operations / task filesystem context / umask

## Problem / Context

Anemone约定`vfs_*`表示不隐式绑定当前进程的kernel-internal VFS operation，面向用户线程且需要current task
语义的operation使用`kernel_*`。`kernel_*`不要求机械调用同名`vfs_*`，可以在不复制VFS owner invariant的前提下
组合多个context-free primitive。

合入umask后，live creation path只部分符合该边界：`openat`、`mkdirat`与`mknodat`在syscall adapter读取umask，
`fs/vfs`内的private helper却继续读取current credentials来形成uid/gid、parent SGID与`CAP_FSETID`结果。于是一次
user creation policy横跨syscall和VFS两层，boot/KUnit等internal creator也依赖偶然current task。继续把umask
下沉会扩大这种穿透；留在各syscall又会让future creator复制formation规则。

本轮保存这项局部owner纠偏，因为代码和测试不能低成本解释为什么current-context止于kernel operation、为什么
`kernel_*`不需要一对一转发，以及为什么SGID admission必须先于umask。target、owner、failure/cleanup和验证可在
一个原子checkpoint闭合，因此不建立RFC。

## Decision

- task filesystem context继续唯一拥有umask及fork copy、`CLONE_FS` share、exec preserve生命周期；credential
  truth仍由task credentials拥有。
- syscall adapter只做用户参数读取与Linux ABI normalization；`kernel_*` creation operation取得一次
  operation-local credential checker和umask snapshot，用同一checker完成pathname search、DAC、capability与
  inode formation。
- umask只在实际创建regular file/tmpfile/directory/make-node时应用一次；打开已有对象与symlink不应用。
- non-directory SGID admission先检查原始requested `S_ISGID|S_IXGRP`、parent SGID、group membership与
  `CAP_FSETID`，再应用umask。否则`umask(S_IXGRP)`会掩盖requested group-execute并错误保留SGID。
- creation-facing VFS primitive只消费显式parent/name/final permission/uid/gid或`MakeNodeDescription`，仍维护
  writable mount、backend dispatch与dentry materialization，但不得查询current task、credentials、`FsState`、
  fd table、user pointer或raw Linux ABI。
- kernel-internal pathname creator使用具名`vfs_touch_as_root`、`vfs_mkdir_as_root`、`vfs_symlink_as_root`，或直接
  传入explicit owner；`kernel_*`可以自由组合VFS primitive，不为命名对称增加空转发层。

## Change

本轮Implementation Boundary只覆盖现有`openat(O_CREAT/O_TMPFILE)`、`mkdirat`、`mknodat`、`symlinkat`及其直接
internal creator。user-visible mode/owner/errno、umask state lifecycle、backend commit/dentry cleanup、
`O_TMPFILE` stage-1语义与public Rust/Linux ABI必须保持；POSIX default ACL、pathname Unix Socket、common-create
publication atomicity、lwext4 crash atomicity和全仓命名清理均不在本轮。

实现新增owner-local `fs::api::creation`，以`KernelCreationPolicy`保存窄的operation-local snapshot，并把四个
syscall路径改为对应`kernel_*` operation。Task增加explicit-checker parent lookup入口，保证lookup、DAC与formation
使用同一credential snapshot。VFS exact primitive改为显式接收final owner/permission，make-node继续通过
`MakeNodeDescription`完成backend handoff；internal caller机械切换为explicit-root语义。

实现反馈中的focused LTP第一次发现`creat09`在`umask(S_IXGRP)`下仍保留SGID。对照Linux 6.6.32
`fs/namei.c#vfs_prepare_mode`与`fs/inode.c#mode_strip_sgid`后确认根因是formation先应用umask、后检查
`S_ISGID|S_IXGRP`。本轮将顺序修为SGID stripping先于umask，并增加直接KUnit；这保持accepted target且关闭了一个
实际privilege-bit错误，不改变Iteration Boundary。

## Validation

- `just fmt all --check`通过；临时probe删除后的user-test分别通过riscv64与loongarch64 release build。
- `just build --preset qemu-virt-rv64-release --bind smp=1 --bind memory=1G`：sandbox内lwext4 C compile触发
  `Bad system call`，完全相同命令在sandbox外通过。
- `just build --preset qemu-virt-la64-release --bind smp=1 --bind memory=1G`通过；两架构build串行执行，未把共享
  `build/generated/device-tree/platform.dtb`的并行结果作为证据。
- RV64端到端运行327项boot KUnit全部通过；LA64运行332项全部通过。两架构temporary mknodat mode probe均确认
  requested `06777`在umask `0027`下得到`06750`；probe、专用LTP group与raw userspace wrapper在取证后删除。
- 两架构glibc/musl的`umask01`、`mkdirat01/02`、`mknodat01/02`与`symlink01`通过。LA64及RV64 glibc的
  `creat09`在ext2/ext3覆盖两种umask并确认SGID被清除；RV64 musl在测试环境user/group lookup处以
  `EAFNOSUPPORT`提前中断，LA64 ext4在既有mount限制处中断。
- focused profile还暴露既有非目标缺口：`open14`缺少`O_TMPFILE` relink、`openat04`因noacl而TCONF、
  `symlinkat01` case 8因absolute pathname仍提前解析invalid dirfd而失败；本轮没有把这些结果写成通过。
- source audit确认`anemone-kernel/src/fs/vfs`内`get_current_task`、`FsPermChecker::for_current_fs`与
  `mask_creation_perm`引用为零，且临时`vfs-creation-policy` wiring/raw wrapper均已删除。
- 独立只读change review核对live source、current contract与已有日志，Apollyon、Keter、Euclid finding均为零；
  review未重新运行build或QEMU。

## Remaining Risk / Links

- 尚无专门stress覆盖`CLONE_FS` peer在creation operation进行中并发修改umask；当前由operation-local snapshot
  代码形状、生命周期KUnit与contract约束。
- errno runtime证据覆盖主要LTP组合，但不声称穷举invalid mode、dirfd、read-only mount与DAC的全部交叉矩阵。
- POSIX default ACL仍未实现；未来ACL owner必须让default ACL与直接umask stripping互斥，不能叠加两次。
- common-create backend/cache/dentry failure window继续由
  [ANE-20260801-VFS-CREATE-PUBLICATION-ATOMICITY](../../register/open-issues.md#ane-20260801-vfs-create-publication-atomicity)
  跟踪；本轮未改变backend commit或cleanup边界。
- lwext4 arbitrary I/O failure/crash atomicity继续受
  [ANE-20260801-VFS-MAKE-NODE-LWEXT4-ATOMICITY](../../register/current-limitations.md#ane-20260801-vfs-make-node-lwext4-atomicity)
  限制；normal-runtime metadata-before-publication规则不变。
- Current contract：[VFS Creation与Make Node](../../contracts/vfs/make-node.md)。
- Historical inputs：[umask文件创建掩码](./2026-07-27-umask-file-creation-mask.md)、
  [VFS Make Node R2](../../rfcs/vfs-make-node/index.md)及其
  [completed transaction](../transactions/2026-07-31-vfs-make-node.md)。
- 外部源码证据：`xref:linux-6.6.32:fs/namei.c#vfs_prepare_mode`、
  `xref:linux-6.6.32:fs/inode.c#mode_strip_sgid`。
- Runtime logs：`build/vfs-creation-policy-rv64.log`、`build/vfs-creation-policy-la64.log`（build-local，未入库）。

## Contract Impact / Cutover

| Contract ID | 变化 | Cutover 前 effective baseline | 新 effective 规则 | 生效证据 |
| --- | --- | --- | --- | --- |
| `VFS-CREATION-001` | Introduce | umask state在task filesystem context，但creation policy分散在syscall adapter与current-dependent VFS helper | current-task admission/formation止于`kernel_*` operation；VFS exact primitive只消费显式facts，internal caller显式owner | 本记录的source audit、KUnit、双架构build/runtime与同一commit |
| `VFS-MAKE-NODE-001` | Refine | syscall读取umask，VFS读取current credentials并拥有parent/DAC/formation与backend handoff | syscall只normalize raw ABI；kernel make-node operation拥有capability、lookup、DAC与formation；VFS拥有context-free backend/dentry handoff | 同上 |

代码、测试、本记录与[current contract正文](../../contracts/vfs/make-node.md)在同一checkpoint原子生效；没有
transitional双路径。历史umask change record与Closed RFC继续保存各自当时的真实baseline，不作为当前规则正文。
