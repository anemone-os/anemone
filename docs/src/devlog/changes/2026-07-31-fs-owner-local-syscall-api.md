# ANE-CHG-20260731-fs-owner-local-syscall-api

**Type:** Cleanup
**Status:** Completed
**Date:** 2026-07-31
**Authors:** doruche, Codex
**Area:** fs / syscall adapter / module ownership

## Problem

`fs::api`同时保存VFS-wide syscall adapter和已经具有明确子模块owner的eventfd、timerfd、iomux、epoll与socket
adapter。后者必须通过`pub(crate)`或`pub(in crate::fs)`访问位于sibling module中的core能力，使源码目录无法表达
“Linux UAPI只属于对应owner的边界层”，也让后续修改容易继续扩大`fs::api`与core之间的可见面。

## Scope

本次只迁移五个已经具有稳定owner的adapter：

- `eventfd2`归入`fs::eventfd::api`，并将单文件eventfd core目录化；
- `timerfd_create/gettime/settime`归入`fs::timerfd::api`，并将单文件timerfd core目录化；
- `ppoll/pselect6`及共同的temporary-mask收尾helper归入`fs::iomux::api`；
- `epoll_create1/ctl/pwait/pwait2`归入`fs::epoll::api`；
- IPv4 UDP socket syscall adapter归入`fs::socket::api`。

`fs::api`继续拥有尚未单独解析owner的VFS-wide/general入口。本次不整理close/dup、mount、pipe/splice、read/write，
不建立`net::api`，不移动`net`实现，不增加compatibility re-export，也不改变syscall ABI、errno、blocking、readiness、
opened-description lifecycle或注册号。

## Solution

adapter成为其semantic owner的private child module。owner core只向child或同owner parent暴露所需能力；epoll仍作为
独立持久对象owner与iomux同级，仅通过`fs::iomux::api`的窄helper复用temporary signal-mask收尾协议。该形状保留
“iomux拥有一次wait round，epoll拥有persistent watch/ready lifecycle”的边界，并避免目录嵌套暗示iomux拥有epoll
状态。

本次`Contract Impact: None`。所有effective contract ID与规则正文均Preserve；四份current contract只机械刷新实现/
Enforcement locator。历史RFC、background、tracking issue与completed transaction中的旧路径保持历史原文。

## Change

本次冻结write set为：

- `anemone-kernel/src/fs/{eventfd,timerfd,epoll,iomux,socket}/`中的owner module、owner-local `api/`与必要visibility；
- `anemone-kernel/src/fs/api/mod.rs`及被上述owner替代的旧adapter路径；
- `anemone-kernel/src/fs/mod.rs`中的timerfd/socket module visibility；
- epoll、iomux、signal temporary-mask与Network UDP Socket current contract中的live implementation locator；
- 本记录、change index、`SUMMARY.md`与当前双周devlog。

迁移保持所有syscall adapter函数体不变；eventfd/timerfd/epoll/socket中仅由旧sibling adapter使用的core surface由
crate/fs-wide收紧到owner subtree。`fs::api`不保留旧路径re-export，避免形成第二套长期入口。

## Validation

- `just fmt kernel --check`通过。
- `just build --preset qemu-virt-rv64-release --bind smp=1 --bind memory=1G`：sandbox内`lwext4` C compile命中
  `Bad system call`，完全相同命令在sandbox外通过；
- `just build --preset qemu-virt-la64-release --bind smp=1 --bind memory=1G`通过；
- live Rust path扫描确认旧`fs::api::{eventfd2,iomux,socket,timerfd}`引用为零，且新owner-local module均被owner
  `mod.rs`直接纳入；
- 最终LA64 `build/anemone.elf`的`llvm-nm --defined-only`输出包含15个迁移入口的`__SYSCALL_*` registration
  symbol，且module path全部位于新的owner-local `api`；
- `git diff --cached --check`、新文件独立whitespace检查与`mdbook build docs`通过；mdBook只保留既有large
  search-index warning；
- 独立subagent review确认代码层Apollyon/Keter/Euclid为0；其发现的current-contract locator Keter与本记录closure
  Euclid已按本节和四份current contract修正。

## Tracking Issues

### CHG-001 - Syscall registration reachability

**Status:** Neutralized
**Severity:** Keter

**Issue:** adapter文件移动后若owner没有直接声明private `api` module，syscall registration可能在编译成功的同时从最终
链接中消失；不能只以文件存在或rename相似度作为证明。

**Resolution:** 五个owner都直接声明其`api` module，双架构canonical build通过，最终ELF保留15个新路径registration
symbol，live Rust旧路径扫描为零；final review确认未丢入口或重新扩大adapter surface。

## Risk / Follow-up

本次不声称`fs::api`已经完成分类；general/VFS-wide入口与close/dup、mount、pipe/splice等候选owner需要独立判断。
若后续清除fs外对`fs::api`的最后依赖，可以单独评估把顶层`pub mod api`收紧为crate-private或private；本轮不把该
公共可见性变化与目录迁移捆绑。

## Links

- Biweekly devlog: [2026-07-20至2026-08-02](../2026-07-20_to_2026-08-02.md)
- Current contracts: [epoll](../../contracts/epoll/index.md), [iomux](../../contracts/iomux/index.md),
  [Network UDP Socket](../../contracts/net/udp-socket.md)
- Register / limitations: 无新增条目
- RFC / transaction: 历史文档保持不变
- 外部源码证据：无
