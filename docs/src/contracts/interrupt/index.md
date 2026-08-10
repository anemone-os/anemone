# Interrupt Flow当前契约

**Contract ID：** `IRQ-FLOW-001`
**状态：** Active
**Owner：** IRQ core拥有单次dispatch transaction；irqchip拥有controller protocol选择与controller state；设备driver拥有device-side cause
**参与领域：** `exception/intr/irq` / concrete irqchip / registered device driver
**覆盖范围：** 已映射、已启用的外部设备IRQ从controller claim/ack到handler与controller completion的顺序
**不覆盖：** CPU timer/IPI、shared/free/threaded IRQ、affinity/SMP routing、storm recovery、polarity、runtime removal或handler panic恢复
**实现位置：** `anemone-kernel/src/exception/intr/irq/`、`anemone-kernel/src/driver/intc/`、注册IRQ的concrete device drivers
**依赖：** None
**Pending Successor：** None
**最后核验：** 2026-08-10

## `IRQ-FLOW-001` - Controller flow与device cause handoff

**规范性规则：** electrical trigger与controller transaction是两份不同事实。irqchip在成功翻译firmware interrupt
specifier时必须显式选择该mapping的controller flow；generic IRQ core不得仅根据edge/level trigger推断controller
操作。当前有效flow及其正常dispatch顺序为：

- edge-ack：`ack -> device handler`；
- level：`mask -> device handler -> eoi -> unmask`；
- fast-eoi/claim-complete：root controller先完成claim，随后`device handler -> eoi/complete`，不得为得到complete而
  伪装成generic level mask/unmask。

IRQ core拥有从已解析descriptor进入flow到handler正常返回后完成controller transaction的单次dispatch。对一个
成功claim且已映射的source，正常返回路径必须exactly once执行所选flow要求的每个controller操作。当前mapping在
descriptor发布后才unmask；翻译失败、重复request或descriptor建立失败不得启用source。当前不支持`free_irq`或
runtime removal，因此mapping与handler能力持续到reset/power-off。

在 descriptor 建立前，firmware-node / IRQ resource owner 必须按唯一的 interrupt name 或合法 index 选择
恰好一个 firmware specifier；name 缺失、重复、长度不匹配、index 越界或 specifier 截取失败都必须在
mapping/unmask 前返回错误。irqchip 只接收选中的单项 specifier，device driver 不解析 controller raw cells，
也不把当前硬件 IRQ 数值提升为 ABI。该规则由 JH7110 GMAC 的 `macirq` production consumer 在最终
VisionFive 2 验收中完成 cutover。

device driver唯一拥有device-side cause。启用中断的driver必须在handler内、任何尾部eoi/complete/unmask之前清除、
消费或以设备协议认可的方式撤销cause；IRQ core和irqchip不得猜测设备寄存器语义。纯polling设备或尚未实现
device-side enable/ack/disable的功能不得注册空handler占位。若设备在completion后仍保持cause，controller再次投递
是正确行为，不由generic core自动屏蔽或退避。

handler panic、未知claimed hwirq或descriptor不一致是kernel-fatal correctness failure；本契约不承诺unwind、
fail-close completion或继续运行。root claim drain上界、shared handler dispatch和storm containment需要独立设计，
不得从本规则推导。

**当前irqchip映射：**

- SiFive PLIC：firmware trigger保留level-like事实，controller flow为fast-eoi；claim完成ack，handler后complete；
- LA7A PCH-PIC/EIOINTC：level flow；handler先撤销设备level，随后EIO pending clear与PCH/EIO reopen；
- Loongson 2K1000 ICU：DMA edge source选择edge-ack，其余当前level source选择level flow。

**违反表现：** PLIC为获得complete而经过generic level mask/unmask；level handler未清设备cause便重新开放；一次
正常dispatch漏掉或重复ack/eoi/unmask；polling-only或未实现IRQ功能的driver注册空handler；QEMU模型偶然不重投被
误作协议正确性证据。

**验证 / enforcement：** `irq/flow.rs`的recording irqchip KUnit直接执行production executor并逐项比较edge、level、
fast-eoi operation trace；每个concrete `InterruptInfo`构造点接受source audit，确保trigger与flow均由irqchip owner
给出；所有`request_irq()` caller审计device-side cause责任。RV64/LA64 canonical wrapper分别以VirtIO-Net external
RX/TX覆盖PLIC fast-eoi与PCH level真实QEMU接线、KUnit和bounded shutdown。QEMU不能替代hardware、SMP或2K1000
runtime proof。

**Cutover前baseline：** IRQ core只保存trigger，并由edge/level固定推断flow；PLIC把source翻译成level以获得
complete，Goldfish RTC在alarm未实现时仍注册空handler。该baseline从未形成stable current contract ID。

**最初来源 / 当前来源：** [IRQ flow protocol小迭代](../../devlog/changes/2026-07-31-irq-flow-protocol.md)；
JH7110 GMAC `JH7110-GMAC-CUTOVER`（2026-08-10）。

## 当前接受边界

- 当前runtime proof覆盖RV64 QEMU SiFive PLIC与LA64 QEMU PCH-PIC/EIOINTC上的单CPU VirtIO-Net traffic，
  以及用户确认的 VisionFive 2 JH7110 GMAC named-`macirq`/device-cause production path；`smp>1`、
  2K1000 runtime和其它irqchip/device组合仍Not Run。
- controller flow不代替设备driver的cause协议；未来RTC alarm、异步block或新driver必须自行证明enable/ack/disable。
- 当前一次CPU external interrupt入口只claim一个source；bounded drain、公平性和storm containment不在本契约内。
