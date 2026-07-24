# ANE-CHG-20260724-mount-fstype-source-compat

**Type:** Small Feature / VFS ABI Compatibility / Contract Extraction
**Status:** Completed
**Date:** 2026-07-24
**Authors:** doruche, Codex
**Area:** VFS / legacy mount syscall / filesystem registry / procfs / anemone-rs / system targets

## Problem

普通 BusyBox `mount -t proc proc /proc` 会把第一个 `proc` 作为 non-null source、把第二个 `proc` 作为 fstype。Anemone原先把 procfs注册为 `procfs`，同时只把 null source解释为 pseudo filesystem；因此 canonical `proc` lookup先以 `ENODEV`失败，改用 `procfs` 后 non-null source又会被误作 block-device path。

仓库内 `user-test` 通过 `mount(None, "/proc", "procfs")` 绕过了两层错误。相同 workaround也存在于 devfs/ramfs caller和 `anemone-rs` 高层 plain-mount wrapper中；filesystem是否需要设备则没有唯一 owner，syscall以 `ramfs`字符串特判，backend又各自匹配 `MountSource`。

## Scope

本轮只闭合 explicit-fstype plain new mount admission：canonical `proc` identity、filesystem-owned no-device / block-device requirement、typed backend handoff、high-level wrapper与 in-tree caller迁移，以及 2K1000 / VisionFive 2 BusyBox install script source形状。现有 bind/move/remount/topology、mount attr matrix、unmount cleanup、filesystem-private data、`/proc/filesystems`、省略 `-t` probe、network/file/UUID/LABEL source和 scoring fstype bridge均不在本轮。

## Solution

`FileSystemMountOps` 用 `NoDevice` / `BlockDevice` tagged callback把 source requirement与 backend可接收输入绑定为同一真相源。syscall只通过即时派生的窄查询分类：no-device忽略合法 raw source label并向 VFS传 `Pseudo`；block-device要求路径解析到已注册 block handle。`FileSystem::mount()` 在统一 dispatch前以普通 `assert!` 检查 variant/source handoff，backend不再解释 raw source policy。

procfs registry identity改为 Linux canonical `proc`。所有 in-tree caller同步迁移，因此不保留 `procfs -> proc` legacy alias；raw `procfs` 不归一化，按 unknown fstype返回 `ENODEV`。既有 `tmpfs` / `ext2` / `ext3` / `vfat -> ramfs` scoring bridge仍只停在 syscall adapter，继续由 current limitation记录。

`anemone-rs::os::linux::fs::mount` 仍只表达 `flags=0`、`data=NULL`，但 source收窄为必选 `&Path`；底层 raw syscall wrapper保持 nullable word形状。这个边界避免 app继续依赖 Anemone-only null-source约定，也没有把高层 wrapper扩张成完整 mount API。

## Change

- procfs以 `proc` 注册；`/proc/<tgid>/mounts` 的 fstype列与 no-device source列继续从唯一 `FileSystemOps::name`派生，block source列仍使用 `dev(<devnum>)`。
- procfs、ramfs、devfs、anonymous fs注册为 no-device callback；ext4注册为 block-device callback。no-device callback不再接收 `MountSource`，ext4 callback只接收解析后的 block handle。
- new-mount admission先完成 alias/data/registry/KERNEL_FS检查，再按 filesystem source requirement解析 source；block-source拒绝日志记录 canonical fstype、source kind、source空值分类、失败原因与 errno。
- KUnit补充 canonical `proc` / raw `procfs` 无 alias边界，并锁定 loop data在 unknown/KERNEL_FS fstype前拒绝的既有 errno precedence。
- `user-test` 与 `tty-test` 的全部 high-level mount caller改为 canonical non-null source；procfs caller使用 `proc` / `proc`。
- 2K1000与 VisionFive 2 install script使用 `mount -n -t devfs devfs /dev` 和 `mount -n -t proc proc /proc`，删除旧 null-source workaround说明。
- 建立并激活 `VFS-MOUNT-ADMISSION-001..003` current contract；`-001`是 canonical identity baseline的 Refine，`-002`是 typed source-owner规则的 Introduce，`-003`是 Closed mount RFC alias-containment边界的 Preserve / baseline extraction。

## Validation

- `just fmt kernel --check`、`just fmt user-test --check`、`just fmt tty-test --check`通过。
- `just build --preset qemu-virt-rv64-release`、`just app build --arch riscv64 user-test`、`just app build --arch riscv64 tty-test`通过。
- 最终候选使用 `./scripts/run-user-test-rv64.sh etc/preliminary/images/sdcard-rv.img build/mount-fstype-source-compat-rv64.log`运行；wrapper重建 rootfs、复制只读 master到 worktree-local runtime disk、构建 pretest kernel并正常返回 exit 0。
- RV64 guest中255项 KUnit全部通过；迁移后的 non-null devfs/ext4/ramfs/`proc` caller完成 pre-chroot和 chroot后环境初始化，并进入唯一 `tmp` profile。
- glibc与 musl各只执行 `mount01..07`一次；每套 whole-case summary均为 attempted=7、passed=4、failed=3、infra_failed=0、skipped=0。逐 case结果一致：`mount01` 10 TPASS；`mount02`因 `mknod()` `ENOSYS`为1 TBROK；`mount03` 12 TPASS / 28 TFAIL；`mount04` 1 TPASS；`mount05` 8 TPASS；`mount06` 8 TPASS；`mount07` 48 TPASS / 12 TFAIL。
- `mount03`失败仍是已登记的 mount attrs/statfs缺口，`mount07`失败仍是 `MS_NOSYMFOLLOW`缺口；`mount02`是相邻 `mknod`能力缺口。没有 source/fstype admission回归，两个 runtime root合计 whole-case attempted=14、passed=8、failed=6、infra_failed=0。
- validation-only `ltp/profile.txt` 与 `ltp/groups/tmp.txt` 在 run后精确恢复，未进入 cutover diff。
- source audit确认 direct high-level caller仅为 `user-test` / `tty-test`并已全部迁移；raw `procfs`无 alias；`anemone-rs`低层 wrapper、`/proc/filesystems`与 mount topology/lifecycle均未改变。
- LA64 kernel/app/runtime、`tty-test` runtime、独立 BusyBox CLI smoke、2K1000 / VisionFive 2 build/runtime、physical hardware与 broad LTP均为 Not Run；RV64证据不得外推。

## Tracking Issues

None。实现与 public contract在同一 checkpoint闭合，没有留下本记录内部的 active review issue。

## Risk / Follow-up

- `ANE-20260619-MOUNT-FSTYPE-ALIAS-BRIDGE`继续记录 scoring alias，不代表真实 tmpfs/ext2/ext3/vfat支持。
- mount attrs/statfs、`MS_NOSYMFOLLOW`和 `mknod`缺口继续由既有 register边界负责，本轮没有为了提高 LTP结果而静默接受。
- `/proc/filesystems`与省略 `-t` 的 filesystem probe等待真实 consumer/acceptance requirement后另行设计；本轮不预留 projection API。
- system-target脚本已完成 source-level closure，但板级 build/runtime未运行。

## Links

- Biweekly devlog: [2026-07-20 至 2026-08-02](../2026-07-20_to_2026-08-02.md)
- Current contract: [VFS Mount Admission](../../contracts/vfs/mount-admission.md)
- Register / limitations: [mount fstype alias bridge](../../register/current-limitations.md#ane-20260619-mount-fstype-alias-bridge)
- RFC / transaction: [mount-tree-legacy-api RFC](../../rfcs/mount-tree-legacy-api/index.md)；本轮不建立 transaction
- External source evidence: [BusyBox mount.c at oscomp testsuite 8b58dd16](https://github.com/oscomp/testsuits-for-oskernel/blob/8b58dd16d26d30f7c74d48d5832d870d3051b703/busybox/util-linux/mount.c)；`xref:linux-6.6.32:fs/proc/root.c#proc_fs_type`；`xref:linux-6.6.32:include/linux/fs.h#FS_REQUIRES_DEV`
- Issue / PR / commit: current workspace diff; no commit created
