# ANE-CHG-20260731-irq-flow-protocol

**Type:** Bugfix / Small Feature
**Status:** Completed
**Date:** 2026-07-31
**Authors:** doruche, Codex
**Area:** irq / irqchip / VirtIO / RTC

## Problem

Network UDP Stage 5修复LA64 PCH-PIC/EIOINTC delivery后，同步polling的VirtIO block仍注册空IRQ handler，
设备侧cause无人清除，level INTx在controller完成并重新开放后立即重入。删除该注册消除了风暴，但同一空
handler此前在RV64 QEMU下没有形成同样的紧密重入：SiFive PLIC的QEMU模型只在输入线收到新的assert事件时置
pending，claim清除pending后不会仅因线路仍保持high而立即重新置位。

这暴露了IRQ core的独立问题：当前descriptor只保存edge/level electrical trigger，并据此固定选择
`ack -> handler`或`mask -> handler -> eoi -> unmask`。PLIC被翻译成level只是为了获得complete，导致
electrical trigger与controller transaction protocol混为一体；RV64偶然不风暴不能证明该抽象正确。

Goldfish RTC alarm尚未实现，却仍注册另一个空IRQ handler。它当前没有已启用的device-side source，但会给未来
alarm路径留下同类错误前提。

## Scope

本次只完成一个原子IRQ flow cutover：

- IRQ mapping同时保存electrical trigger与irqchip显式选择的controller flow；
- production提供edge-ack、level-mask/eoi/unmask与fast-eoi/claim-complete三种flow；
- SiFive PLIC选择fast-eoi，LA PCH-PIC选择level，Loongson 2K1000 ICU按既有source配置选择edge或level；
- 删除Goldfish RTC在alarm未实现时的空IRQ注册；
- 将原`exception/intr/irq.rs`按同一IRQ owner拆为`irq/{mod.rs,flow.rs}`，flow KUnit留在语义文件末尾。

本次不增加shared IRQ、`free_irq`、runtime removal、threaded IRQ、affinity/SMP routing、root-domain bounded
drain、storm自动屏蔽/退避/watchdog、polarity模型、VirtIO block异步化或RTC alarm。`IrqHandler`与
`request_irq` API保持不变。

## Solution

`InterruptInfo`分别携带`IrqTriggerType`与`IrqFlowType`。每个真正发布IRQ mapping的irqchip在`xlate()`中
显式返回flow，IRQ core不再从trigger推断controller操作。descriptor保存该选择，统一production executor按以下
顺序调用controller与device handler：

- edge-ack：`ack -> handler`；
- level：`mask -> handler -> eoi -> unmask`；
- fast-eoi：root irqchip先claim，随后`handler -> eoi/complete`，不额外mask/unmask。

每次成功映射的dispatch在handler正常返回时exactly once完成所选controller transaction。device-side cause仍由
设备driver唯一拥有：启用中断源的driver必须在handler中、controller completion之前清除或消费cause；纯polling或
尚未实现中断功能的driver不得用空handler占位。handler panic属于kernel-fatal路径，不建立unwind/cleanup语义。

flow executor使用recording irqchip KUnit验证exact operation trace，而不是复制一份选择逻辑。双架构QEMU只作为
真实接线与进展证据：RV64 external VirtIO-Net traffic覆盖PLIC fast-eoi，LA64同源traffic覆盖PCH level flow；
bounded wrapper completion排除紧密风暴，但不把QEMU解释为真实硬件electrical-level证明。2K1000没有本轮runtime
平台证据，只由source policy、KUnit与LA64编译覆盖，明确保持hardware Not Run。

本次仍属于small change：target、owner、failure、cleanup、write set和验证均已解析，没有probe、过渡contract、
第二checkpoint或滚动stage。若实现需要改变handler/request API、引入shared/free/threaded IRQ、第二cutover，或双
架构不能由同一原子实现收口，则停止并升级RFC。

## Change

本次原子cutover已完成，write set为：

- `anemone-kernel/src/exception/intr/irq/{mod.rs,flow.rs}`（替换原`irq.rs`）；
- `anemone-kernel/src/driver/intc/{sifive_plic,loongson_platic,loongson_2k1000}.rs`；
- `anemone-kernel/src/driver/rtc/goldfish.rs`；
- 本记录、IRQ current contract、contract/summary/change-index导航与当前双周devlog。

## Contract Impact / Cutover

| Contract ID | 变化 | Cutover 前 effective baseline | 新 effective 规则 | 生效证据 |
| --- | --- | --- | --- | --- |
| `IRQ-FLOW-001` | Replace（首次提取） | IRQ core仅按edge/level推断flow；device cause责任未形成current contract | irqchip显式选择controller flow；device handler在completion前清cause；正常dispatch exactly-once完成transaction | 本原子checkpoint的source、KUnit、双架构build/QEMU与review |

本次代码、测试与[IRQ当前契约](../../contracts/interrupt/index.md)只在同一commit生效。任一exact-order KUnit、RV64/
LA64 build、canonical runtime或final review失败时，`IRQ-FLOW-001`保持Not Cut Over，不提交部分contract或部分架构
实现。

## Validation

- production flow executor的三个recording irqchip KUnit分别命中edge、level、fast-eoi真实执行路径，严格验证
  `ack -> handler`、`mask -> handler -> eoi -> unmask`、`handler -> eoi`及fast-eoi无mask/unmask；RV64与LA64
  runtime均为277/277 KUnit PASS；
- source audit确认PLIC、PCH-PIC与2K1000三个mapping owner显式选择flow；`request_irq()` consumer只剩实际处理
  device cause的VirtIO-Net与NS16550A，Goldfish RTC不再注册空handler；
- `just fmt kernel --check`、RV64/LA64 release build、`git diff --check`与`mdbook build docs`通过。首次RV64
  wrapper误用final image，因缺少pretest LTP fixture得到`attempted=0 skipped=4`，不计入acceptance；随后两架构
  均使用对应preliminary sdcard master的build-local副本完成canonical wrapper；
- RV64与LA64均完成external UDP guest 17/17、host peer PASS、epoll 11/11、LTP whitelist 4/4以及filesystem →
  network → device → PowerOff序列；RV64自然退出，LA64在既有无有效poweroff handler的halt点由QEMU monitor退出，
  两个wrapper均返回0；
- 独立subagent review确认production code Apollyon/Keter/Euclid均为0；review时仅有的文档closure Keter与缺失
  双周devlog Euclid已由本次最终evidence write-back neutralize。

## Tracking Issues

### CHG-001 - Exact flow oracle

**Status:** Neutralized
**Severity:** Keter

**Issue:** 双架构QEMU success本身不能区分旧PLIC level flow与新fast-eoi；必须由命中production executor的
recording irqchip KUnit给出判别性顺序证据。

**Resolution:** 三个KUnit直接执行production flow executor，并在RV64与LA64的277/277 runtime中给出判别性
operation trace证据。

### CHG-002 - Atomic architecture closure

**Status:** Neutralized
**Severity:** Keter

**Issue:** PLIC fast-eoi与LA level flow必须由同一source完成双架构runtime；单架构通过不得激活contract。

**Resolution:** 同一exact source的RV64与LA64 canonical wrapper均返回0，且final review无production finding；
`IRQ-FLOW-001`随本原子commit完成cutover。

## Risk / Follow-up

- QEMU runtime只证明当前emulated controller/device接线和bounded progress；真实硬件、SMP与2K1000 runtime均Not Run。
- 设备保留cause时，fast-eoi或level completion后再次进入是正确硬件行为；本次不引入storm detector来掩盖
  driver未ack的错误。
- RTC alarm后续实现必须同时建立device-side enable/ack/disable语义，不得恢复空IRQ handler。

## Links

- Biweekly devlog: [2026-07-20至2026-08-02](../2026-07-20_to_2026-08-02.md)
- Current contract: [IRQ flow protocol](../../contracts/interrupt/index.md)
- Register / limitations: 无
- RFC / transaction: [Network UDP transaction](../transactions/2026-07-29-net-udp.md)
- 外部源码证据：无（QEMU模型结论用于问题归因，不作为Anemone contract authority）
- Issue / PR / commit: 本次原子commit
