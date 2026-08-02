# ANE-CHG-20260802-kernel-symbolization

**Type:** Small feature / kernel diagnostics and build orchestration
**Status:** Completed
**Date:** 2026-08-02
**Authors:** doruche, Codex
**Area:** kernel debug / backtrace / symbol table / xtask

## Problem / Context

Kernel backtrace已经能够在RV64与LA64上捕获frame-pointer chain，但每个frame只显示raw return PC。
开发者需要保留完全相同的ELF再离线解析，panic现场本身不能提供函数名与offset。仓库根原有的`symtab`
原型没有consumer，且通过native struct layout和无长度裸指针解释数据；损坏count、offset或结尾字符串可能
越界，lookup也会把函数间gap误归属给最近的低地址symbol。

符号表不能简单在一次link后生成再公开：嵌入新section会推动后续layout，而最终backtrace必须消费与
公开`build/anemone.elf`完全一致的text symbol mapping。本轮需要在不改变unwind、panic/System Power
handoff或userspace ABI的前提下，闭合安全格式、生成owner、link稳定性与runtime loadability。

## Decision

- 根目录`symtab`只拥有private、versioned、little-endian byte format，以及checked host encoder和
  `no_std`、无分配的`&[u8]` parser/binary-search lookup。entry显式记录start、size和length-delimited
  UTF-8 name；只有`start <= pc < start + size`才命中。
- xtask只选择已定义、非零size、位于`SHF_ALLOC | SHF_EXECINSTR` section的`STT_FUNC`。排除`.L*`；
  同地址alias依次按较大size、`GLOBAL > WEAK > LOCAL`、raw-name lexical order canonicalize，并在host
  侧demangle Rust name。
- `kernel_symbols` KernelConfig feature决定能力，Cargo profile不参与policy。tracked默认配置显式开启；
  与其它Cargo feature一致，缺失键按`false`处理。关闭时保持普通单链接和raw-PC formatter；开启时dev与
  release都执行同一有界双链接。
- discovery pass先嵌入canonical empty table，只产生private intermediate。xtask据其生成完整表后执行
  final pass；随后逐项比较discovery、final ELF与embedded table，并校验section、linker range、readonly
  non-executable `PT_LOAD`、runtime `__srodata..__erodata` mapping及所有symbol的executable range。失败不
  自动开始第三次link，也不提交新的public ELF。
- `.anemone.symtab`是两个architecture linker script中的独立4 KiB对齐section，位于`__etext`之后，
  起点同时定义为`__srodata`。这既保持section为只读、不可执行，也避免rodata页表mapping遗漏该section或
  覆盖text末尾部分页的执行权限。
- `CapturedFrame`仍只保存raw PC。formatter保留原PC输出，以`pc.saturating_sub(1)`派生symbol、offset与
  size；missing、empty或malformed table回退raw PC。panic path不分配、不加锁、不建立lazy mutable cache，
  不访问filesystem、VFS、task或userspace。
- public map先发布，verified final ELF作为本次build action的commit point。map失败不能先提交新ELF。

本轮不增加`/proc/kallsyms`、syscall、sysfs/debugfs/device接口、DWARF/line/inline frame、KASLR、
crash dump或userspace address-exposure policy。private byte format不是Anemone userspace ABI。

## Change

- 重写`symtab`并加入root workspace与`just test symtab`，覆盖round-trip、malformed input、overflow、排序、
  UTF-8和exact/inside/end/gap lookup。
- xtask新增ELF producer、canonicalization、双link orchestration、final verification、verified-only
  publication和action-local size diagnostics；discovery ELF/map/generated blob保持在`build/generated/`。
- RV64与LA64 linker input新增`.anemone.symtab`及start/end symbols，并把它纳入现有kernel rodata mapping。
- kernel debug owner新增immutable embedded-table view和symbolized `CapturedBacktrace::Display`；owner-local
  inline KUnit覆盖live function、crafted formatter、lookup boundary及malformed/raw fallback。
- `conf/.defconfig`、board与focused logging KernelConfig显式开启`kernel_symbols`；没有修改current contract、
  register、native syscall表或`anemone-abi`。

## Validation

- `just test symtab`：5/5通过；`just test xtask`：72/72通过。覆盖ELF筛选/alias/demangle、invalid UTF-8、
  discovery/final drift、section/load/runtime-rodata boundary、public artifact失败顺序，以及feature选择一或两次
  link。
- RV64 explicit tuple、release、SMP=8、4 GiB双链接通过：5,801 entries、600,562 string bytes、
  739,818 total bytes。fresh rootfs QEMU运行347/347 KUnit，三项新增backtrace case均`ok`；userspace
  `get-set-invalid-permission-restore` smoke通过并正常PowerOff。
- 同一RV64 KernelConfig的dev双链接通过：61,824 entries、8,926,995 string bytes、10,410,803 total
  bytes，证明能力不由profile决定。
- LA64 explicit tuple、release、SMP=1、4 GiB双链接通过：5,356 entries、578,559 string bytes、
  707,135 total bytes。fresh rootfs QEMU运行352/352 KUnit，三项新增case与同一userspace smoke通过；
  orderly shutdown后因既有LA64 platform没有成功poweroff handler进入halt，terminal PASS后由host终止QEMU。
- 一个validation-only KernelConfig显式设置`kernel_symbols=false`，RV64 release canonical build只出现一次
  `Compiling kernel`并通过；final ELF只有零长度linker output section，不消费非空table，backtrace raw
  fallback由feature-gated source path闭合。
- RV64/LA64 build严格串行。sandbox内canonical lwext4 compile命中`Bad system call` / `SIGSYS`；完全相同
  command在sandbox外成功，因此分类为host validation environment限制。
- 独立reviewer发现并推动修复public ELF早于map提交的问题；复核最终format、producer、link/runtime mapping、
  panic path、profile/feature与publication边界后，没有未关闭的Apollyon、Keter或Euclid finding。
- `git diff --check`通过。`just fmt all --check`剩余差异仅为既有vendored
  `anemone-kernel/crates/anemos/virtio-drivers/src/device/sound.rs` rustdoc drift，本轮修改的Rust文件无formatter
  diff。`mdbook build docs`通过。
- Production panic注入、hardware、完整user-test/LTP/competition profile、DWARF/line symbolization与
  crash-consistent host artifact publication：**Not Run**。

## Remaining Risk / Links

- runtime parser在每次backtrace格式化前线性验证整张table，随后每帧binary search；它不分配也不加锁，
  但本轮没有建立panic latency benchmark。dev profile的表显著大于release，size由每次build诊断而非配置
  ceiling拥有。
- public ELF是build action commit point；本轮覆盖ordinary validation/map publication failure，没有把
  host process/power loss下的多文件publication提升为crash-consistent transaction。
- Production panic继续调用同一`CapturedBacktrace::Display`路径，source audit证明会使用已验证formatter；
  本轮没有为了制造panic增加长期probe。
- Current contract / register / limitation: None；`SYSTEM-POWER-EMERGENCY-001`只是未变化的dependency。
- RFC / transaction / external source: None；implementation evidence由本change的Git commit与上述命令拥有。
