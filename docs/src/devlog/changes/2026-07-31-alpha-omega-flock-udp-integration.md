# ANE-CHG-20260731-alpha-omega-flock-udp-integration

**Type:** Integration / Cleanup
**Status:** Completed
**Date:** 2026-07-31
**Authors:** doruche, Codex
**Area:** VFS flock / Network UDP / opened-description lifecycle / syscall adapter

## Problem

`dev/drc/alpha`在`8013ee4a`完成Network UDP R0及后续局部整理，`dev/drc/omega`在`9bc6ab45`
完成Flock R0与anonymous-inode kind整理；共同base为`d0bfea73`。两条分支的Git文本冲突只有十处，
但合流同时触及opened-description terminal release、anonymous file kind、userspace wrapper、pretest rootfs与
文档导航，不能按`ours`/`theirs`机械选边。

合流检查还发现owner-local syscall adapter的文件名没有始终写完整syscall名，且flock syscall adapter尚未使用
稳定的`api/{mod.rs,flock.rs}`目录形状。

## Scope

本轮只完成以下事项：

- 以alpha为唯一merge receiver，用保留双亲的no-ff merge吸收omega，并在完成后让omega fast-forward到同一基线；
- 语义组合Flock与UDP已经生效的owner、lifecycle、ABI、测试和文档事实，不改变任一accepted target；
- 将flock adapter整理为`fs/flock/api/{mod.rs,flock.rs}`；
- 让FS内注册syscall的源码文件使用完整syscall名，覆盖epoll、fanotify、timerfd，并补齐扫描发现的`umount2`；
- 保留历史RFC、background与completed transaction中的旧源码路径，只刷新current contract的live implementation
  locator和本次集成记录。

本轮不改变flock grant语义、UDP Endpoint/Socket状态、syscall ABI/注册号、errno、blocking/readiness、rootfs内容
意图或既有current contract规则，也不新增通用lifecycle observer、file-lock framework或compatibility re-export。

## Solution

opened-description lifecycle继续只有`ProcFile`一个terminal owner。首次`Live(1) -> Retired`后，owner先执行固定的
flock retirement handoff，完成grant cleanup与recheck notification submission，再调用创建时固定的
`FileDescOps::final_release`。因此UDP、epoll或fanotify的feature-specific cleanup不会覆盖flock cleanup，也没有形成
第二个lifecycle truth或动态callback registry。

VFS file kind采用加法合并：ordinary anonymous control fd使用`InodeType::Anon`并向Linux投影零`S_IFMT`与
`DT_UNKNOWN`；UDP socket使用`InodeType::Socket`并投影`S_IFSOCK`与`DT_SOCK`。ext4对两种anonymous-only kind均
明确拒绝持久化。`anemone-rs`同时保留flock/linkat与UDP socket API，双架构pretest rootfs同时安装`flock-test`和
`udp-test`，user-test本地阶段按flock、epoll、UDP顺序运行。

十个文本冲突按owner处理：`getdents64.rs`保留Anon/Socket两种投影；epoll/eventfd接入合流后的anonymous kind与
lifecycle；`anemone-rs`两份旧`linux.rs`同时保留双方子模块/API；双架构rootfs保留双方app；`SUMMARY.md`、change
index与transaction index保留双方导航。全部冲突都能由既有owner和contract唯一决定，没有需要新增产品、ABI或
acceptance选择的分支。

syscall文件整理只改变同owner private module的物理路径：`epoll_create1`、`epoll_ctl`、`epoll_pwait`、
`epoll_pwait2`、`fanotify_init`、`fanotify_mark`、`timerfd_create`、`timerfd_gettime`、`timerfd_settime`与
`umount2`均使用完整syscall名；`wait.rs`、`abi.rs`等不注册syscall的helper不受此命名规则约束。

本次`Contract Impact: None`。FLOCK、OPENED-DESC、Network UDP Socket、EPOLL/IOMUX与VFS file-kind current
contract语义全部Preserve。

## Change

- alpha parent：`8013ee4a3fe6646aa26d958f0c9dd710a110b593`；
- omega parent：`9bc6ab459d57cad7fb70dfa788f67c587ed8eb9e`；
- merge base：`d0bfea73d2e2cbdceade492870de38928e62d7b1`；
- flock adapter建立owner-local `api`目录；FS syscall registration文件完成完整名称扫描与重命名；
- Flock current contract的live implementation locator刷新到新目录；历史RFC与transaction路径保持原文；
- 本记录、change index、`SUMMARY.md`与当前双周devlog增加集成入口。

## Validation

- `just fmt kernel --check`通过；
- `flock-test`与`udp-test`分别完成RV64/LA64 app build；
- `just build --preset qemu-virt-la64-release --bind smp=1 --bind memory=1G`通过；
- 完全相同范围的RV64 release build在sandbox内编译未修改的`lwext4`时命中`Bad system call`，在sandbox外用
  `just build --preset qemu-virt-rv64-release --bind smp=1 --bind memory=1G`通过，归类为sandbox限制；
- 最终LA64 ELF包含flock以及重命名后的epoll、fanotify、timerfd、`umount2` syscall registration symbol，module
  path均指向新文件；
- FS源码扫描确认所有包含`#[syscall(SYS_...)]`的`.rs`文件名都等于完整syscall名；
- 最终集成候选通过`just fmt all --check`、`git diff --cached --check`与`mdbook build docs`。

本轮未运行rootfs materialization、QEMU guest、KUnit、LTP或hardware；父分支已有runtime证据仍是历史证据，不能
替代合流candidate的runtime验证。

## Tracking Issues

### CHG-001 - Terminal release形成两个cleanup owner

**Status:** Neutralized
**Severity:** Keter

**Issue:** 若flock retirement与UDP/epoll/fanotify `final_release`互相覆盖或顺序不确定，final close可能遗留grant或
feature publication，并形成并列lifecycle truth。

**Resolution:** `ProcFile::release_description_ref()`在唯一terminal transition中固定先执行flock retirement，再执行
创建时固定的feature hook；fd-table guard已经释放，两条路径均不重定义published-ref truth。source audit与双架构
build通过。

### CHG-002 - Anonymous control fd与socket共享错误kind

**Status:** Neutralized
**Severity:** Keter

**Issue:** 只保留任一分支的`InodeType`新增项会让另一类对象错误投影为ordinary anon或socket，破坏`stat`/`statx`
与`getdents64`可见ABI。

**Resolution:** `Anon`与`Socket`作为两个独立internal kind同时保留，Linux mode与dirent projection各自唯一；ext4
exhaustive consumers显式拒绝二者的anonymous-only持久化路径。

## Risk / Follow-up

- 本次集成没有新的register项；既有MM COW stack-overflow等开放问题不属于本次合流根因。
- rootfs、QEMU、KUnit、LTP与hardware均为Not Run。若需要把“双方功能在同一guest中共同工作”作为新验收结论，
  应在共同基线上另行运行双架构canonical wrapper；本次不从build成功外推该结论。
- syscall文件名规则只约束实际注册syscall的文件；不要把shared helper机械拆成每个syscall的重复实现。

## Links

- Biweekly devlog: [2026-07-20至2026-08-02](../2026-07-20_to_2026-08-02.md)
- Current contracts: [Opened-description lifecycle](../../contracts/task/opened-description-lifecycle.md)、
  [VFS flock](../../contracts/vfs/flock.md)、[VFS file kind](../../contracts/vfs/file-kind.md)、
  [Network UDP Socket](../../contracts/net/udp-socket.md)
- RFC / transaction: [Flock R0](../../rfcs/flock/index.md)、[Network UDP R0](../../rfcs/net-udp/index.md)
- Register / limitations: 无新增条目
- 外部源码证据：无
