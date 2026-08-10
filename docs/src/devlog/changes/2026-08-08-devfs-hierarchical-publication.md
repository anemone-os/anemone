# ANE-CHG-20260808-devfs-hierarchical-publication

**Type:** Small Feature / kernel-internal namespace capability
**Status:** Completed
**Date:** 2026-08-08
**Authors:** doruche, Codex
**Area:** devfs / device publication / VFS projection

## Problem / Context

devfs 已经通过一个 persistent superblock 向所有 mounts 投影同一组静态设备节点，但 namespace 仍是扁平的
root registry。root 与静态 `/dev/shm` directory 使用不同表示，directory 本身没有 child namespace，内核
subsystem 因而不能取得窄能力后组织多级静态布局。继续为 mountpoint 或设备类别增加 basename 特判会保留
root/child 两套 inode 与 enumeration 路径，也不能为未来 `/dev/pts` 等稳定 mountpoint 提供一般能力。

本轮只补齐 publish-until-reboot 的层级 publication。运行期 unpublish/hotplug、provider teardown、alias、symlink、
inode/dentry reclaim 与 devpts/PTY lifecycle 仍需要独立协议。

## Decision

devfs 保持一个 production namespace 和一个 persistent superblock 的系统单例。namespace 改为 append-only tree；
每个 directory 的单一 ordered child collection 同时拥有 basename admission、lookup 与 readdir order，不建立 root-global
registry 或 map/vector 双重真相。

向 kernel subsystem 暴露 opaque `DevfsDirectory` capability，只允许在其代表的 directory 下发布一个 direct child
directory 或 leaf。它不接受完整路径，也不暴露 VFS inode、dentry、superblock、lock guard 或 private child container。
既有 root `publish()` 继续作为兼容入口，但委托 production root capability；`/dev/shm` 也走普通 directory publication。

每个 node 只保存一次 stable inode identity、immutable parent inode identity、kind/attributes 与 leaf ops。root 与 nested
directory 共用同一 inode/file operation path；`.`、`..`、link count、lookup、readdir、open 与 getattr 均从 node 和
children 投影。child 不强持有 parent，避免 append-only tree 与 isolated validation namespace 形成引用环。

## Implementation Boundary

Target 是在不改变 userspace ABI、既有 leaf behavior、singleton/multi-mount identity 和 publish-until-reboot lifetime 的
前提下，支持任意有限深度的静态 devfs hierarchy。`fs::devfs` 唯一拥有 production namespace、inode allocation、
directory children 与 publication commit；producer subsystem 继续拥有 `DevfsNodeOps` behavior 与 provider lifetime；
VFS 只消费 inode lookup/readdir projection。

publication 在 namespace lock 外构造 fallible node/inode；parent lock 内重新检查 duplicate、预留 ordered collection、
seed 完整 inode，并在 directory child 情况下更新 parent nlink projection，最后以 child insertion 作为唯一可见
linearization point。插入后没有 recoverable failure step；插入前失败只留下不可见的 monotonic inode-number hole，
不会改变 children、order、nlink 或 icache reachability。rejected producer capability 也不会在 namespace lock 内析构。

本轮不增加 userspace mutation、recursive path API、unpublish/replacement/rename、hard link、alias/symlink、per-mount
namespace、devpts/PTY、scheduler/wait dependency、production probe 或 test-only mount facade。`Contract Impact` 为
`None`；本轮依赖并保持
[`TTY-ENDPOINT-001`](../../contracts/tty/data-plane.md#tty-endpoint-001--endpoint-publication是稳定的单向transaction)
的 prepare-before-publish 与 publish-until-reboot 规则。

## Change

- 新增 devfs namespace/node owner 与 opaque `DevfsDirectory`，把 inode allocator 和 singleton superblock 收入同一
  namespace core；legacy root publication 与 `/dev/shm` 委托一般 direct-child path。
- root 与 nested directory 收敛到统一 inode/file ops；immutable parent inode identity 驱动 `..`，single ordered child
  collection 驱动 lookup/readdir，directory nlink 是该 collection 的 VFS metadata projection。
- publication 使用 lock 外 prepare、lock 内 admission/reserve/seed/metadata/insert 顺序；append-only readdir cursor 继续
  使用稳定 index。
- 两个 isolated inline KUnit 场景覆盖 hierarchy lookup/readdir、`.`/`..`、nlink、leaf attrs/open、stable inode identity、
  parent-local duplicate/admission 与 namespace 整体释放；既有 production mount tests 继续覆盖 singleton/multi-mount 与
  char/block leaf behavior。

## Validation

- 独立 subagent final review 首轮发现两个 closure blocker：并发 publication 可能让 directory `getattr` 在不同 children
  snapshot 间误触 nlink assertion，以及 duplicate/failure cleanup 可能在 namespace lock 内析构 producer-owned ops。
  实现改为在同一 children read guard 内计算并核对 nlink，并把 fallible prepare 与 rejected candidate drop 移出 lock；
  follow-up review 确认两项已 neutralized，且无残余 Architecture Friction finding。
- `./scripts/run-final-test-rv64.sh etc/final/images/sdcard-rv.img
  build/devfs-hierarchical-publication-rv64.log` 在最终代码上运行：586/586 KUnit 通过；新增
  `test_devfs_hierarchy_projection` 与 `test_devfs_parent_local_admission` 均为 `ok`；日志出现
  `All tests passed!`，随后真实 init 完成 devfs 与 `/dev/shm` 接线并到达 `busybox-init: started shell 139`。
  维护者确认验收后手动关闭 QEMU。
- closure 另执行 repository-owned kernel formatting/check、`git diff --check` 与 `mdbook build docs`；这些检查不重跑
  kernel/QEMU，最终结果由本 commit 的 Git evidence 保存。
- **Not Run:** LA64 build/runtime、LTP、完整 competition suite、concurrent publication stress、fault injection、hardware、
  provider teardown、unpublish 与 inode/dentry reclaim validation。

## Remaining Risk / Links

- append-only publication 仍要求 producer ops/endpoint capability 稳定到重启。真实 hotplug/unpublish 必须另行定义
  namespace invalidation、open handle、dentry/inode、enumeration cursor 与 provider teardown protocol。
- directory capability 只提供静态层级表达能力，不定义 devpts 的 per-slave allocation、grant/unlock、revoke/hangup、
  ownership/mode 或 mount-instance semantics。
- [`ANE-20260524-DEVFS-STATIC-PUBLISH`](../../register/current-limitations.md#ane-20260524-devfs-static-publish)
  已收窄为上述动态失效、别名及回收缺口；block default semantics 与 TTY/PTY/devpts 其它条目不因本轮关闭。
- RFC / transaction：None。
