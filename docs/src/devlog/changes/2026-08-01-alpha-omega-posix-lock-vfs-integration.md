# ANE-CHG-20260801-alpha-omega-posix-lock-vfs-integration

**Type:** Integration / Cleanup
**Date:** 2026-08-01
**Authors:** doruche, Codex
**Area:** VFS make-node / POSIX record lock / development workflow / pretest

## Problem / Context

`dev/drc/alpha`在`1a75deab`完成VFS Make Node R2，`dev/drc/omega`在`bff3aba3`完成POSIX
record lock R0及开发工作流收敛；共同base为`0dea02d5`。两条分支的九个文本冲突集中在VFS inode/module
聚合、LTP profile、current contract和文档导航，但双方已经分别完成target、runtime、review与contract
cutover，合流不能通过`ours`或`theirs`丢失任一侧的有效事实。

合流还需要证明：alpha的inode目录化不会让omega重新引入旧`fs/inode.rs`或第二个lock owner；omega的新工作流
不会追溯改写VFS Make Node的Closed RFC/Completed transaction；默认pretest不安装或直接执行开发期
`fcntl-test`时，双架构`user-test`仍能以tracked `sys` profile启动并正常结束。

## Decision

alpha作为唯一merge receiver，以保留双亲的no-ff merge吸收omega。VFS继续采用alpha的
`fs/inode/{mod.rs,metadata.rs,object.rs,ops.rs}`形状；POSIX range domain作为inode identity拥有的唯一grant与
conflict truth加入`inode/object.rs`。flock与POSIX record lock统一位于private `fs::lock::{flock,posix}`，但保持
两个独立advisory namespace，不建立共同grant engine或兼容re-export。

工作流采用omega已经生效的`anemone-development-workflow`与`docs/src/development-workflow.md`，旧
`anemone-rfc-doc-workflow`入口删除。新规则只作用于新任务和活跃RFC的下一个未开始gate；VFS Make Node与POSIX
record lock的Closed RFC、Completed transaction及历史路径保持原文。本次只建立一份小迭代记录，不再同步双周
devlog。

`fcntl-test`仍保留为可显式构建和运行的focused product suite，但在RV64/LA64 pretest manifest中保持注释状态，
`user-test`中的直接调用同步注释，避免rootfs未安装二进制却在启动后无条件执行。tracked LTP profile保持`sys`，
专用`posix-record-lock` group仍可供后续显式验证。

本次`Contract Impact: None`。合流只组合两侧已经生效的current contract；device number、file kind、flock与
POSIX record lock页面仅刷新live implementation locator，不改变stable ID、owner、规则、来源或effective状态。

## Change

- alpha parent：`1a75deab07289da8b302f7a860af444e898d553b`；
- omega parent：`bff3aba3fc77f5e4b43ff0469c1cfe2e6125f724`；
- merge base：`0dea02d5b23d3a0eb6d2ec8b4345d28a149dbefc`；
- 九个Git冲突按上述owner与文档权威做加法合并；旧monolithic `fs/inode.rs`不进入最终树；
- VFS inode object接入`PosixLockDomain`，`fs::lock`收口flock/POSIX private module namespace，同时保留
  alpha的make-node callback与`reject_make_node`路线；
- 双架构pretest禁用`fcntl-test`安装与直接调用，最终profile固定为`sys`；
- 合并双方RFC、current contract、register与导航，并采用新的最小artifact开发工作流。

## Validation

- `just test xtask`通过，62/62；`just fmt all --check`、`git diff --cached --check`与
  `mdbook build docs`通过；
- `fcntl-test`与`user-test`分别完成RV64/LA64 app build；
- `qemu-virt-rv64-release`与`qemu-virt-la64-release`使用`smp=1 memory=1G`串行完成kernel build；RV64在sandbox
  内编译未修改的lwext4时命中`Bad system call`，相同命令在sandbox外通过，归类为sandbox限制；
- RV64/LA64 canonical pretest均重建不含`fcntl-test`的rootfs并进入`user-test`，302项KUnit全部通过，包含
  make-node、POSIX range domain、file-table episode与cleanup覆盖；
- 两种架构的glibc/musl `sys` profile均为attempted 4、passed 4、failed 0、infra failed 0、skipped 0，日志包含
  `All tests passed!`；RV64由guest正常退出，LA64完成filesystem/network/device shutdown后因既有平台无可用
  power-off handler进入terminal halt，再由host结束QEMU。

Hardware与`fcntl-test` focused product suite未在本次合流candidate上运行；app build和集成KUnit不替代这些
runtime证据。

## Remaining Risk / Links

- 本次没有新的register项或accepted limitation；VFS Make Node与POSIX record lock的既有开放边界
  继续由各自RFC、current contract和register拥有。
- Current contracts：[VFS make-node](../../contracts/vfs/make-node.md)、
  [VFS POSIX record lock](../../contracts/vfs/posix-record-lock.md)、
  [File-table POSIX lock](../../contracts/task/file-table-posix-lock.md)。
- RFC / optional transaction：[VFS Make Node R2](../../rfcs/vfs-make-node/index.md)、
  [POSIX Record Lock R0](../../rfcs/posix-record-lock/index.md)、
  [VFS Make Node transaction](../transactions/2026-07-31-vfs-make-node.md)、
  [POSIX Record Lock transaction](../transactions/2026-07-31-posix-record-lock.md)。
- 外部源码：`None`。
- Issue / PR / commit：本次双亲merge commit。
