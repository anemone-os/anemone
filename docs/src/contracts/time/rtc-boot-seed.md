# RTC Boot Seed 当前契约

**Contract ID：** `RTC-BOOT-SEED-001`
**状态：** Active
**Owner：** device RTC core（registration / selection / read）、machine description（preference）、boot coordinator（handoff）与 common timekeeper（offset commit）
**参与领域：** device discovery、concrete RTC driver、machine description、boot coordinator、timekeeper
**覆盖范围：** boot-only RTC provider registration、确定性选择、一次读取、2K1000非法日历初始化与 realtime offset seed
**不覆盖：** devfs/ioctl、alarm/IRQ、通用/关机/运行期RTC writeback、runtime read/resync、hotplug/unregister、fallback chain、suspend correction 与 NTP
**实现位置：** `anemone-kernel/src/{device/rtc,driver/rtc,arch/*/machine,main.rs,time/timekeeper.rs}`
**依赖：** [`TIMEKEEPER-CLOCK-001`](./clock-derivation.md#timekeeper-clock-001--所有-clock-读取来自一条整数推导链)、[`TIMEKEEPER-STEP-001`](./realtime-step.md#timekeeper-step-001--realtime-step-不能漏掉或误用旧-timeline)
**最后核验：** 2026-08-18

## 状态与能力所有权

| 状态 / 能力 | 唯一 Owner | 其它参与方持有什么 | 行为用途 |
| --- | --- | --- | --- |
| RTC MMIO、寄存器访问与 device state | concrete RTC driver/provider object | RTC core持同一个`Arc<provider>` | 提供一次可失败的Unix Epoch读取；2K1000 provider可执行受限非法日历初始化 |
| provider origins、registry phase、selection与一次read | device RTC core | driver只提交`origin + provider`；boot coordinator只取得epoch sample | 关闭注册窗口并确定至多一个boot source |
| preferred RTC origin | selected machine description | boot coordinator取得一次boot policy value | 用stable firmware identity表达平台选择 |
| discovery/finalization/seed顺序 | common boot coordinator | 不保存provider handle | 在用户态可见前完成一次handoff |
| realtime offset与change sequence | common timekeeper | RTC core只提供epoch sample | 建立唯一calendar projection |

## RTC-BOOT-SEED-001 — RTC 只在 boot handoff 中建立一次 calendar anchor

**规则：** concrete RTC driver完成所有fallible setup后，向device RTC core登记stable firmware origin和同一个
device-owned provider object。duplicate origin拒绝且不覆盖旧entry；driver state与registry必须共享同一provider，
不得复制MMIO mapping或寄存器状态形成第二份hardware truth。

machine description只提供可选preferred origin，不读取registry/provider或修改timekeeper。discovery完成后，boot
coordinator唯一调用finalization：该操作先原子关闭registration window，再按下列规则选择至多一个provider：

1. explicit preference唯一命中时选择它；miss不fallback；
2. 没有preference且恰有一个provider时选择它；
3. 没有provider时保持zero-offset fallback；
4. 没有preference且有多个provider时报告ambiguity，不按registration order选择。

finalization在RTC registry锁外执行至多一次provider read。selected read失败不读取其它provider；late registration和
重复finalization都不能重新打开window或产生第二次seed。finalization后不暴露selected-provider runtime handle。

唯一写入例外属于2K1000 concrete provider：只有firmware node的primary compatible为`loongson,ls-rtc`，且第一次硬件snapshot已经
coherent、但月/日/时/分/秒或2000..2099年份无法组成合法日历时，provider才可把TOY写为
`2001-01-01 00:00:00`。MMIO读取失败或year-boundary snapshot持续不一致不得触发写入；`loongson,ls7a-rtc`
及其它provider不得使用该恢复策略。写入后必须重新使能并检查TOY/oscillator enable bits，再取得coherent
snapshot并解码；只有读回值精确等于目标日历时，provider read才成功返回对应epoch。enable或读回失败均返回
read failure，不向timekeeper交付sample，也不读取其它provider。原日历已经非法，因此失败路径不尝试回滚可能已经
发生的寄存器写入。该行为只发生在RTC core发起的唯一boot provider read内部，不形成任意set-time、关机写回或运行期
同步能力。

成功读取`rtc_epoch_ns`后，timekeeper紧接着采样monotonic，并在一次提交前验证：

```text
boot_offset_ns = rtc_epoch_ns - monotonic_sample_ns
realtime_ns = monotonic_now_ns + boot_offset_ns
```

negative offset或realtime加法overflow使提交整体失败，offset保持原值。成功boot seed只提交初始offset，不增加
`realtime_change_seq`、不产生`RealtimeStep`，也不扫描或通知soft timer/timerfd/wait owner。普通realtime/coarse
读取和后续set/adjust继续只访问timekeeper，不再读取RTC。

**违反表现：** registry或machine layer保存第二份MMIO/current RTC；selection依赖registration order；explicit miss/read
failure后fallback；持registry锁读取MMIO；late probe重开selection；timekeeper保存provider handle；boot seed推进
change sequence或进入runtime step notification；普通clock read再次访问RTC；incoherent/MMIO失败时写RTC；
primary compatible不是`loongson,ls-rtc`的provider却初始化非法日历；未完成enable/readback验证就报告set成功或提交seed；
shutdown/runtime路径写RTC。

**验证 / Enforcement：** inline KUnit覆盖unique/duplicate/late registration、preference hit/miss、zero/one/multiple
selection、read failure no-fallback、一次read/finalize与ordinary clock不重入provider；timekeeper KUnit覆盖normal、negative、
overflow、no-partial-mutation与change sequence不变。2026-08-07 RV64 provider-bearing QEMU通过576/576 KUnit，启动日志
证明Goldfish registration、preference selection、一次read与seed commit；同次`clock_tests`验证2020+ realtime、同offset
coarse projection及后续mutation/restore。2026-08-08 LA64 QEMU的LS7A provider接入后通过578/578 KUnit；同次
`clock_tests`的首次realtime `1786121791006115430ns`落在host日志birth/mtime区间
`[1786121778, 1786121798]s`内，并通过native clock、realtime mutation/restore、soft timer与POSIX timer检查。该LA64运行
随后在无关的gateway ping阶段由调用者终止，因此不作为full user-test或orderly poweroff证据。RV64/LA64 app与release
kernel构建通过。2026-08-18 `2k1000-la64-release`与`qemu-virt-la64-release`构建通过；LA64 QEMU完成785/785
KUnit，包含固定寄存器编码/epoch和legacy-compatible策略两项新增case。2K1000实机写入与读回仍为Not Run，不能从
fake-MMIO KUnit或QEMU外推。

**最初来源：** [RTC Provider 与 Boot Walltime Seed 小迭代](../../devlog/changes/2026-08-08-rtc-provider-boot-walltime-seed.md)。

**当前来源：** 原closure commit中的device RTC core、Goldfish provider、boot handoff与timekeeper seed，后续LS7A
concrete provider Patch，以及[2K1000非法RTC日历启动初始化小迭代](../../devlog/changes/2026-08-18-2k1000-rtc-invalid-calendar-init.md)。
