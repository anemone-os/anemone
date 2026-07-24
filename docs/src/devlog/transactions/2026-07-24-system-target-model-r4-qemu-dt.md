# System Target Model R4A QEMU Provider DT Cutover

**Status:** Completed
**Date:** 2026-07-24
**Owner:** doruche
**Canonical Revision:** [RFC-20260722-system-target-model R4](../../rfcs/system-target-model/index.md)
**Implementation Authority:** [Checkpoint R4A](../../rfcs/system-target-model/implementation.md#checkpoint-r4a---qemu-provider-dt-cutover)
**Previous Transaction:** [R3 explicit-input cleanup](./2026-07-24-system-target-model-r3-explicit-inputs.md)
**Contract Impact:** None；[`BOOT-PROTOCOL-001`](../../contracts/task/boot-protocol.md) Preserve

## Scope and authorization

R4确认QEMU machine model是可执行machine-fact authority，不再需要仓库中的provider-derived DTS mirror及
refresh/check维护面。用户授权完成唯一Ready checkpoint R4A，并要求避免过度工程；R4B始终保持Outline，
本事务不解析或激活它。R4A只改DT schema/materialization/CLI及对应文档，不改变Platform/SystemTarget/
BuildPreset identity、kernel handoff、ordinary QEMU bind argv、physical DTS内容或current contract。

## Activation preflight

Preflight读取live source、Platform/schema、Justfile与CLI help、R4 canonical docs、register/current
limitations、R0-R3 transaction及build-system skill。Disposable topology probe分别比较RV64/LA64 core、
ordinary fixed args与完整三bind attachment；去除唯一允许的`/chosen/rng-seed`后，每架构三份canonical DTS
逐字节相等，没有命中R4A topology-neutral停止条件。Frozen write set足以完成cutover。

## Change

- Platform `dtb.source`改为optional，authority类型收口为`DtAuthority`；resolved Platform在任何action
  side effect前拒绝QEMU/physical/provider/source及RV64/LA64 delivery非法组合。
- Build DT materializer按完整Platform分派：firmware delivery删除stale output；physical embedded继续用
  committed DTS和`dtc`；QEMU embedded只使用固定程序、machine、CPU、SMP、memory及optional BIOS执行
  `dumpdtb`。Provider output必须是regular file并通过FDT header/size检查，随后原子发布；失败清理temporary
  与final output。
- 删除`qemu dt`、refresh/check、drift专用status、canonicalization/diff/write-back及对应测试；ordinary
  QEMU、bind validation、`--show-bindings`与architecture -> fixed program owner保持。
- 四份QEMU Platform删除`dtb.source`，两份QEMU-derived DTS删除；physical Platform/DTS、U-Boot output与
  runtime delivery不变。Schema/example、配置说明、build skill及两份reference同步。

## Validation and review

最终代码字节的`just xtask-test`为55 passed / 0 failed，覆盖DT contract矩阵、delivery反向组合拒绝、
firmware stale cleanup、physical `dtc`、QEMU topology-only argv、provider failure/missing/invalid output与
atomic publish。`just xtask qemu --help`不再暴露子命令；`dt`、`refresh`和`--check`均以Clap status 2拒绝，
RV64/LA64 `--show-bindings`仍显示原三项声明。

六份release build串行通过：RV64/LA64 QEMU bare与pretest、VisionFive 2及2K1000。LA64 provider命令固定为
`qemu-system-loongarch64 -machine virt,dumpdtb=... -cpu la464 -smp 1 -m 1G`，没有BIOS、ordinary args或bind；
published raw DTB保留`rng-seed`且只有一个CPU。LA64后接RV64 build证明firmware path删除stale DTB。
VisionFive继续生成U-Boot image，2K1000继续编译committed DTS并发布raw output。首次沙箱内RV64 build仅因
lwext4 C compile的seccomp `SIGSYS`失败，同命令在沙箱外重跑通过。

两架构topology-neutral probe通过。Draft 7 Platform schema及全部7份TOML、production residual/source、
两份wrapper `bash -n`、`just fmt all --check`、`git diff --check`与`mdbook build docs`通过；mdBook只有既有
large search-index warning。

RV64既有成功boot证据覆盖OpenSBI、255项KUnit、init与user-test；该窗口随后进入当前LTP profile，因此
LTP输出不计入R4A acceptance。一次更短复跑在既有`openat` KUnit出现`AlreadyExists` panic，用户要求停止
继续测试并确认以成功boot证据收口；本事务不把该现象归因于R4A。LA64 runtime按用户明确指示Not Run。
Physical runtime、完整LTP与final harness同样Not Run。

Full diff与latest-byte review确认修改均位于冻结manifest，physical owner、ordinary QEMU execution、ABI与
contract边界未漂移：Apollyon 0 / Keter 0 / Euclid 0。Accepted limitation `STM-R4-S1`保持。

## Closure

R4A及本transaction Completed，最终形成一个`target-model: ...` checkpoint commit。R4仍为
Accepted for Implementation；R4B保持Outline。本closure不运行、解析或授权
`R4A -> R4B Implementation Resolution Gate`。Contract cutover为None，`BOOT-PROTOCOL-001` Preserve。
