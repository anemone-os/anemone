# ANE-CHG-20260609-user-test-staged-tools

**Type:** Test Infra Improvement
**Status:** Completed
**Date:** 2026-06-09
**Authors:** doruche, Codex
**Area:** anemone-apps/user-test / competition runner / fixture staging

## Problem

`user-test` 现有 fixture 能力主要服务 LTP 文本环境，依赖编译期字符串和 shell heredoc
写入测试盘。这个路径不适合承载 `mkfs.ext4` 这类 ELF 工具；而 LTP 和部分 competition
case 会先检查或调用这些工具，再进入真正要验证的内核路径。

如果测试盘缺少工具，日志容易把“测试材料不足”混进 VFS、block、loop、mount、mmap、
statx 或 quota 语义失败里。本轮需要一个二进制安全的通道：启动盘先 stage 工具，
`user-test` 在挂载测试盘后、`chroot()` 前把工具复制进测试盘。

## Scope

本轮只增加第一版 staged test-tool 通道：

- 在 competition disk 挂载到 `/mnt` 后安装 staged 文件；
- 在 `chroot("/mnt")` 前复制，因为进入 chroot 后启动盘 staging 路径不可见；
- runner manifest 只保留 `source` 和 `dest`；
- 支持当前 rv64 测试使用的 `mkfs.ext4` 命令路径；
- 缺少已列入 manifest 的 source 时 fail closed，并打印明确的 source / dest 缺口。

本轮不做目录同步、包管理、压缩包展开、版本校验、依赖解析，也不使用
`include_bytes!` 把二进制 payload 编进 runner。

## Solution

`user-test` 新增一个很窄的 staged fixture manifest。每个条目只描述启动盘 source
和测试盘内 dest。installer 接收挂载点 `/mnt` 后，创建目标父目录，并用
`openat` / `read` / `write` 复制字节。工具权限由 staged asset 和 rootfs staging
配置负责；runner 不在 pre-chroot 阶段调用 BusyBox `chmod`，也不额外修复既有目标权限。

安装点必须在 pre-chroot 边界内完成。进入 `chroot("/mnt")` 后，启动盘上的
`/fixtures/...` 已经不可见；继续依赖该路径会把传输机制和 chroot 后运行环境混在一起。

第一轮 rv64 通过启动盘 `/fixtures/user-test/tools/mkfs.ext4` stage 工具，再安装到
测试盘 `/bin/mkfs.ext4`。测试用例看到的是 Linux 兼容命令名。loongarch64 本轮没有
对应 staged 工具资产，因此 manifest 暂为空；等有匹配资产后复用同一 source/dest 通道。

## Change

- `anemone-apps/user-test/src/main.rs`
  - 新增 `StagedCompetitionFixture { source, dest }`。
  - 在 `/dev/vdb` 挂载到 `/mnt` 后、`chroot("/mnt")` 前调用
    `install_staged_competition_fixtures("/mnt")`。
  - 新增基于 `openat`、`read`、`write` 的二进制安全复制 helper。
  - 已列入 manifest 的 staged source 缺失时，在进入 chroot 前失败。
- `anemone-apps/user-test/staged/riscv/mke2fs`
  - 提供 rv64 静态工具资产，由本地 rootfs manifest stage 到启动盘工具通道；资产侧
    保持可执行权限。

## Validation

Agent-run validation:

- `file anemone-apps/user-test/staged/riscv/mke2fs` 确认为静态链接 RISC-V ELF。
- `ls -l anemone-apps/user-test/staged/riscv/mke2fs` 确认 staged asset 具备可执行权限。
- `just fmt user-test` 通过。
- `just xtask app build user-test --arch riscv64` 通过。
- `just xtask app build user-test --arch loongarch64` 通过。
- agent 侧 `just xtask rootfs mkfs -c rootfsconfig-rv` 已完成 rootfs staging 阶段，并显示
  `Staging file 'anemone-apps/user-test/staged/riscv/mke2fs'`；后续
  `virt-make-fs` / `supermin` 在 agent 主机环境层失败。用户环境可成功构建镜像，因此
  该失败不归类为本轮 rootfs staging 缺口。
- `build/rootfs/minimal-rv/root/fixtures/user-test/tools/mkfs.ext4` 已存在，保留
  `rwxr-xr-x` 权限，并确认为同一个静态 RISC-V ELF。
- `git diff --check -- anemone-apps/user-test/src/main.rs anemone-apps/user-test/staged/riscv/mke2fs` 通过。

尚未运行：

- rv64 端到端 `user-test` / LTP 运行，确认 chroot 后 `/bin/mkfs.ext4` 可见且可执行。

## Tracking Issues

### CHG-001 - loongarch64 staged 工具资产

**Status:** Deferred
**Severity:** Safe

**Issue:** installer 已能在 loongarch64 构建通过，但本轮没有提供 loongarch64
`mkfs.ext4` 资产。

**Resolution:** loongarch64 暂保留空 staged tool manifest；后续有匹配资产后通过同一
source/dest 通道加入。

### CHG-002 - 运行时依赖验证

**Status:** Deferred
**Severity:** Euclid

**Issue:** rv64 工具是静态链接 ELF，但本轮尚未在 guest 内跑完整路径，也未在测试盘内
执行 `mkfs.ext4 -V`。

**Resolution:** 本记录只声明传输机制和 rv64 staged 资产已经就位。命令可见后若继续
失败，应重新归类为工具行为、block / loop / mount / ext4、或 syscall / VFS 语义问题，
不要再归为 missing command 测试设施缺口。

## Risk / Follow-up

- runner manifest 继续保持最小形态。`mode`、`kind`、`required`、版本校验或依赖图
  需要另起小迭代评估。
- 现有 LTP 文本 fixture 可以继续保留在原路径；只有出现明确收益时再迁移到 staged
  binary-safe 通道。

## Links

- Biweekly devlog: [2026-06-08 至 2026-06-21](../2026-06-08_to_2026-06-21.md)
