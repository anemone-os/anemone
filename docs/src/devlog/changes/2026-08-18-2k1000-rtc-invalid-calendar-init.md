# ANE-CHG-20260818-2k1000-rtc-invalid-calendar-init

**Type:** Small feature / scoped RTC contract exception
**Status:** Completed
**Date:** 2026-08-18
**Authors:** EDGW, Codex
**Area:** Loongson RTC / 2K1000 boot / timekeeper seed

## Problem / Context

2K1000实机日志`etc/log-board-la-new-60.log`的三次启动都完成RTC probe和provider selection，但原始TOY值分别为
`0x3def3014`、`0x3def3e64`和`0x3def4ea1`，year均为`16381`。minute/second在重启间继续推进，说明硬件计数器
在走时并保留内容；但month/day/hour字段均无法组成合法日历，Anemone因此拒绝boot seed，用户态只能看到从1970
附近开始的boot uptime projection。

tracked Linux驱动用`TOY_WRITE0_REG=0x24`、`TOY_WRITE1_REG=0x28`顺序写入calendar pair，随后使能TOY和oscillator；
其字段编码与Anemone读取路径一致。参考：`xref:linux-6.6.32:drivers/rtc/rtc-loongson.c#loongson_rtc_set_time`。

## Decision / Implementation Boundary

Target是在primary compatible为`loongson,ls-rtc`的2K1000 provider唯一boot read中，若第一次snapshot已经coherent、但
calendar解码失败，则写入固定值`2001-01-01 00:00:00`，重新使能硬件并读回验证；只有读回精确匹配才返回
`978307200000000000ns`并允许既有boot coordinator给timekeeper seed。成功set只在验证后打印`kinfoln!`。

MMIO/coherent-snapshot失败不写；`loongson,ls7a-rtc`及其它RTC provider不使用该策略。写后enable或读回失败均按
selected-provider read failure处理，不fallback到其它provider、不提交seed；原值已经非法，失败路径不回滚可能已经发生的
硬件写入。RTC MMIO和恢复决策仍由同一个concrete provider拥有，RTC core继续只负责一次provider调用和handoff。

本轮不修改DTS，不增加任意set-time API、devfs/ioctl、alarm/IRQ、关机写回、runtime resync、fallback chain或新的
timekeeper路径。若需要让其它compatible共享恢复、在incoherent read后写入、接受非精确读回或增加运行期/关机写回，必须
重新解析target和`RTC-BOOT-SEED-001`，不能扩张本例外。

## Change

- `Ls7aProvider`保存从firmware primary compatible派生的immutable boot policy；只有`loongson,ls-rtc`允许非法日历初始化；
- Loongson寄存器owner增加固定日期专用写入，不暴露通用写时钟接口；写入`0x04200000`和year-since-1900 `101`，再复用
  既有TOY/oscillator enable检查；
- provider对初始invalid、enable failure、incoherent readback和decoded mismatch分别记录诊断；只有验证成功后打印
  `2K1000 RTC successfully set to 2001-01-01 00:00:00`，随后保持一次最终accepted current-time notice；
- owner-local KUnit验证固定寄存器编码、epoch、enable bits和legacy-compatible策略，既有invalid-calendar拒绝覆盖保持不变。

## Validation

- `just fmt kernel --check`与`git diff --check`通过；
- `just build --preset 2k1000-la64-release`通过，包含KUnit feature的discovery/final双阶段LoongArch构建；
- `just build --preset qemu-virt-la64-release`通过；
- LA64 QEMU使用现有`build/rootfs/2k1000/rootfs.img`走到boot-integrated runner，785/785 KUnit全部通过，两项新增
  Loongson RTC case均PASS。KUnit完成后该rootfs的init退出触发既有`init task shall not exit`并进入PowerOff halt，因此本次
  只作为KUnit runtime证据，不作为完整userspace boot或orderly shutdown证据；
- source review确认写寄存器顺序、字段编码、enable/readback gate、success log位置以及generic RTC core/timekeeper/shutdown
  路径均未扩大。

**Not Run:** 2K1000实体硬件启动、真实TOY写入/读回、第二次重启后的持久走时、用户态`date`、LTP、final harness、
alarm/IRQ和关机路径。fake-MMIO KUnit与LA64 QEMU不能证明2K1000硬件接受写入；实机验收必须观察invalid warning、set成功、
current-time notice、provider read success和boot seed commit，并确认`date`落在2001年而非1970年。

## Contract Impact / Cutover

| Contract ID | 变化 | 先前effective baseline | Effective scoped exception |
| --- | --- | --- | --- |
| [`RTC-BOOT-SEED-001`](../../contracts/time/rtc-boot-seed.md#rtc-boot-seed-001--rtc-只在-boot-handoff-中建立一次-calendar-anchor) | Scoped Exception | selected provider只读取硬件；RTC不写回 | 仅primary compatible为`loongson,ls-rtc`的coherent-invalid boot snapshot可写固定2001日历；enable和精确读回验证后才返回sample，所有其它失败/compatible及runtime/shutdown仍不写 |

`TIMEKEEPER-CLOCK-001`和`TIMEKEEPER-STEP-001`作为受保护依赖不变：timekeeper仍只接收一个validated epoch sample，
不持provider、不增加runtime calendar truth，也不把boot seed变成realtime step。

## Remaining Risk / Links

- 实机尚未运行是唯一关键验证缺口；如果读回存在硬件规定的提交延迟，当前精确即时校验会fail closed且不seed，不会把未验证
  时间报告为成功。只有真实日志证明需要等待机制后，才能在同一owner内加入有硬件依据的bounded polling。
- Current contract：[RTC Boot Seed](../../contracts/time/rtc-boot-seed.md)。
