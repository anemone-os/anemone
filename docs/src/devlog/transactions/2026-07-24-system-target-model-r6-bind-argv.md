# System Target Model R6 Named Bind and Initial Argv

**Status:** Completed
**Date:** 2026-07-24
**Owner:** doruche
**Canonical Revision:** [RFC-20260722-system-target-model R6](../../rfcs/system-target-model/index.md)
**Implementation Authority:** [Checkpoint R6A](../../rfcs/system-target-model/implementation.md#checkpoint-r6a---named-opaque-bindings-and-initial-argv)
**Previous Transaction:** [R4A QEMU provider DT cutover](./2026-07-24-system-target-model-r4-qemu-dt.md)
**Contract Impact:** [`BOOT-PROTOCOL-001`](../../contracts/task/boot-protocol.md) Refine at R6A cutover

## Scope and authorization

用户接受R6 target并授权一个checkpoint内更新RFC、实现、验证和提交。范围只包括xtask具名opaque-string
binding、optional QEMU argv group，以及RootfsEntry/EmbeddedApp共享的完整initial argv。决赛脚本、决赛
Platform/SystemTarget实例、typed/range/path validator、envp配置与rootfs metadata格式变化均不在本事务。

## Activation preflight

Preflight读取live RFC/current contract、register/current limitations、R5 closure、xtask config/resolver/build/
QEMU/DT owner、generated boot input与kernel boot consumer、tracked schema/config及build skill。确认provider的
SMP/memory同时由ordinary QEMU和LA64 build-time DT物化消费，runtime attachment group只由QEMU消费；两类
输入可以共用`--bind NAME=VALUE`词汇但必须按action分别拒绝未消费值。Checkpoint R6A的Ready definition、
write set、validation与停止条件由canonical implementation持有。

## Execution log

R6A已完成。实现统一`{{name}}`replacement与`--bind NAME=VALUE`解析，build只消费provider字段绑定，QEMU
消费provider、fixed args和runtime group绑定；required/optional组、single-pass替换及所有拒绝路径均由
xtask tests覆盖。两种initial-program source共享optional完整argv，省略时保持resolved-path-only默认。

## Validation and review

- `just xtask-test`：55/55通过；全部tracked Platform/SystemTarget TOML通过对应JSON schema验证。
- RV64与LA64 current release build串行通过；LA64 provider命令保持`la464`、`-smp 8`、`-m 1G`。
- `--show-bindings`输出符合具名声明；negative build在side effect前拒绝未消费bind。
- `just fmt all --check`、`git diff --check`与`mdbook build docs`通过；mdBook仅有既存search index体积warning。
- 最终subagent review未发现实现级Apollyon/Keter；报告的一项文档Keter已修复，R4历史provider-bind禁令不再
  作为R6 current invariant。QEMU guest、rootfs、LTP、physical runtime、final harness与决赛脚本Not Run。

## Contract cutover and closure

`BOOT-PROTOCOL-001`已在R6A原子Refine：RootfsEntry与EmbeddedApp都可携带包含`argv[0]`的完整非空argv；
省略时仍以resolved executable path作为唯一argv。Rootfs metadata保持path-only，envp保持固定。
R6A与RFC Closed；没有live tracking issue。决赛脚本、具体决赛Platform/SystemTarget实例、typed/value-aware
validator、generic binding API和可配置envp均未进入本事务。
