# ANE-CHG-20260731-net-udp-external-peer-retirement

**Type:** Cleanup
**Status:** Completed
**Date:** 2026-07-31
**Authors:** doruche, Codex
**Area:** UDP / user-test / pretest harness

## Problem

`net-udp` Stage 5为了形成一次双架构remote-external cutover evidence，在`udp-test`中加入固定
address/port/token roundtrip，并让两个通用`run-user-test` wrapper永久拥有Python host peer、child cleanup、独立日志和
精确17项summary marker。该case只提供一次QEMU provider smoke；继续把它绑定到所有user-test运行会让无关回归依赖
host端口和额外进程，并让后续扩展`udp-test`时必须同步维护wrapper计数。

## Scope

本次只退休专用remote-external guest case、host peer脚本和两套wrapper的peer lifecycle。保留`udp-test` app、pretest
rootfs安装、`user-test`调用及其余focused UDP matrix，后续仍可直接扩展这些case。不修改kernel/protocol/ABI、
SystemTarget/Platform/rootfs composition、R0 target、effective contract规则或已完成RFC/transaction历史。

## Solution

删除固定wire agreement及其唯一consumer，把通用wrapper恢复为rootfs/disk/kernel/QEMU编排。wrapper继续要求
`UDPTEST:SUMMARY:PASS:`，但不再把case数量冻结为脚本接口；`udp-test`自身仍以非零退出阻止`user-test`和normal
shutdown成功。Stage 5 transaction与Git保留remote-external双架构cutover evidence，current contract只把持续
enforcement改写为当前focused matrix，并明确后续external-path回归由进入canonical验证的真实UDP consumer承接。

该清理的Contract Impact为None：owner、handoff、failure、cleanup、ABI与用户可见能力均不变，只缩减长期维护的
validation asset。

## Change

- 删除`udp-test`的`remote-external-roundtrip`及固定peer常量；其余case保持原顺序和行为。
- 删除`scripts/net-udp-echo-peer.py`。
- 删除RV64/LA64 wrapper的peer启动、READY等待、回收、peer日志与remote marker，保留通用guest completion检查。
- 更新UDP Socket current contract的enforcement描述，不改写已完成RFC/transaction中的历史证据。

## Validation

- `bash -n scripts/run-user-test-rv64.sh scripts/run-user-test-la64.sh`通过。
- `just fmt udp-test --check`通过。
- `just app build --arch riscv64 udp-test`与`just app build --arch loongarch64 udp-test`通过；两次均由repository app
  exporter重新构建并发布`build/apps/udp-test/udp-test`。
- source/harness残留扫描确认live `udp-test`与两套wrapper不再引用peer、remote case、Stage 5 marker或固定17项summary。
- `git diff --check`、新文件独立whitespace检查与`mdbook build docs`通过；mdBook只保留既有large search-index warning。
- 未运行rootfs、QEMU、KUnit、LTP或hardware；本次不宣称重新证明external path或guest runtime，只验证删除边界、
  双架构app compile/export与文档一致性。

## Risk / Follow-up

在真实UDP程序进入canonical回归前，仓库不再持续运行一个专用remote-provider roundtrip。该变化不撤销Stage 5已经取得
的cutover evidence，也不表示当前external capability退化；后续consumer的验证声明必须按其实际运行路径重新给出，
不能仅引用本次清理前的peer脚本。

## Links

- Biweekly devlog: [2026-07-20 至 2026-08-02](../2026-07-20_to_2026-08-02.md)
- Current contract: [Network UDP Socket](../../contracts/net/udp-socket.md)
- Register / limitations: 无新增条目
- RFC / transaction: [Network UDP R0](../../rfcs/net-udp/index.md),
  [completed transaction](../transactions/2026-07-29-net-udp.md)
- 外部源码证据：无
