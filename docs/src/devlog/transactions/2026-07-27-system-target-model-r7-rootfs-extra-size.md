# System Target Model R7 Rootfs Incremental Extra Size

**Status:** Completed
**Date:** 2026-07-27
**Owner:** EDGW
**Canonical Revision:** [RFC-20260722-system-target-model R7](../../rfcs/system-target-model/index.md)
**Implementation Authority:** [Checkpoint R7A](../../rfcs/system-target-model/implementation.md#checkpoint-r7a---incremental-rootfs-extra-size)
**Previous Transaction:** [R6 named bind and initial argv](./2026-07-24-system-target-model-r6-bind-argv.md)
**Contract Impact:** None

## Scope and authorization

用户提供2k1000 folder rootfs的`virt-make-fs`导入`ENOSPC`证据，并授权增加`extra-size`字段。R7只改变
rootfs manifest与xtask image materialization surface，不改变kernel/runtime ABI、absolute capacity、
image-base resize或其它rootfs配置。

## Execution log

Rootfs `Fs`新增optional `extra-size`。Folder materialization将值映射为`--size=+<value>`，省略时继续完全
委托`virt-make-fs`自动估算；image base在任何side effect前拒绝该字段。2k1000使用256 MiB增量余量，
以覆盖ext4 metadata和native test build输出，同时保留原估算作为基础。

## Validation and review

- `just xtask-test`运行57项，新增folder parse、image rejection与既有absolute-size rejection测试通过；
  总结果55 passed / 2 failed。两个失败分别是当前工作树既有resolver fixture mismatch与DT `/bin/false`
  error-text断言，不属于R7路径。
- 在`gallant_lamarr`中执行`just rootfs mkfs -c conf/rootfs/2k1000/rootfs.toml`成功；实际命令包含
  `virt-make-fs --size=+256M`。`virt-df`显示ext4为487 MiB、235 MiB已用、217 MiB可用，原失败Git
  pack及其idx/rev通过显式只读挂载检查存在。
- `just fmt all --check`中kernel与xtask通过，随后停在未修改
  `anemone-apps/busybox/src/main.rs`的既有格式差异。`git diff --check`通过。
- Host与`gallant_lamarr`均未安装`mdbook`，文档构建Not Run。

## Closure

R7A完成folder增量余量、image ownership拒绝、2k1000配置和canonical documentation同步。Contract cutover为
None；2k1000实际镜像已在具备libguestfs的Linux环境完成生成和只读内容验证。
