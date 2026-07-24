# RFC-20260722-system-target-model

**状态：** Implemented / Closed
**修订：** R5
**负责人：** doruche
**最后更新：** 2026-07-24
**领域：** build system / configuration / platform / repository workflow
**事务日志：** [R4A QEMU provider DT cutover](../../devlog/transactions/2026-07-24-system-target-model-r4-qemu-dt.md)；
[R3 explicit-input cleanup](../../devlog/transactions/2026-07-24-system-target-model-r3-explicit-inputs.md)；
[R0-R2 implementation history](../../devlog/transactions/2026-07-22-system-target-model.md)
**影响契约：** [`BOOT-PROTOCOL-001`](../../contracts/task/boot-protocol.md#boot-protocol-001--typed-initial-program-source统一收口到普通-vfs-exec)（Refine；Checkpoint 5A已于2026-07-24 cut over并生效）。
**开放问题：** None。
**下一步：** None；hardware-semantic QEMU Platform与用途别名作为独立
[小迭代](../../devlog/changes/2026-07-24-qemu-smp-platform-aliases.md)维护。

## 文档状态

本文是 R5 accepted target 的 canonical source；它不是 current contract 或 resolved write set。
本文把已经形成的 system-target 定位共识展开为实施目标，并取代历史 positioning 文档作为
本 RFC 的 target authority。

本目录同时提供 [迁移实施计划](./implementation.md)，用于约束后续 stage envelope、feedback
gate、停止条件与回写路径。Stage 1-4及其全部checkpoint均已独立关闭；实际执行证据由transaction记录。
Stage 5与单一Checkpoint 5A已独立关闭；`BOOT-PROTOCOL-001`已在5A原子cutover。Stage 6与单一
Checkpoint 6A已完成current workflow回归、审计和生命周期write-back并关闭。R3以新的transaction
清理R2遗留的隐式输入并已关闭，没有重开或改写Completed的R0-R2 transaction。R4接受QEMU DT
provider反馈：QEMU不再提交或维护DTS baseline；firmware delivery直接消费runtime FDT，embedded
delivery由normal build从已解析QEMU provider物化build-local DTB。R4A实现并关闭该DT cutover。R5把
从未进入Ready或Active的R4B adopter迁移移出本RFC；对应Platform配置应用不属于target model本身。

## 摘要

本 RFC 重构 Anemone 的构建与配置模型。目标不是为 final harness 增加一个特殊启动
分支，而是把当前混在 root `kconfig`、platform manifest、rootfs manifest、QEMU
参数和 xtask task 中的机器事实、系统产品语义、kernel capability、平台内核产物、
QEMU invocation input 与构建展示重新分配给唯一 owner。

目标模型以 `Platform`、`SystemTarget`、`KernelConfig`、`BuildPreset` 和不可变
`ResolvedSystemBuild` 为核心。`Justfile` 与 `scripts/xtask` 继续作为仓库构建编排入口；
`build` 生成 selected Platform 要求的 kernel artifacts，其中可以包含确定性的 post-link
格式转换。Rootfs/app 与 execution 保持各自的直接 action，跨 action 编排由用户或现有
wrapper 通过明确命令顺序完成，不引入通用 package 或 artifact graph。

Final harness 只是该模型形成过程中的首个压力来源。它的 runner、评分、测试选择、
镜像版本和兼容策略不属于本 RFC target；在 system-target model 可用后，应作为独立
小迭代或 adopter plan 接入。

## 背景

### 当前配置把多个 owner 混在一起

live build path 当前具有以下事实：

- root `kconfig` 的 `[build]` 同时选择 platform、Cargo profile 和 disassembly，
  其余区段又保存 kernel feature 与参数；
- `conf/platforms/*.toml` 同时描述 architecture、machine constants、root mount、
  QEMU executable/args、host disk path、DTB source 和可选 U-Boot kernel-image 字段；
- `just conf switch` 修改 root `kconfig` 中的 platform 字段，但不会形成不可变的完整
  resolved snapshot；
- kernel build 分别重读 `kconfig` 与 platform manifest，并可能在 prebuild 中启动
  QEMU 生成 DTB；
- QEMU task 再次按 platform 名读取配置，实际执行不证明 kernel artifact 与当前
  platform/KConfig 来自同一次解析；
- VisionFive kernel build 会在 ELF 导出后按 Platform `[uboot]` 隐式生成 legacy image，
  rootfs recipe 再通过固定 `build/anemoneImage-rv64` 路径消费它；这条顺序当前依赖用户按
  约定先运行 kernel build，再运行 rootfs action。

这些不是若干字段放错目录的孤立问题。当前配置同时缺少产品级 boot/deploy owner和
不可变解析身份，导致 pretest/final/physical board 等组合
只能通过复制 platform、修改本地 `kconfig` 或把 host path 写进 machine config 表达。

### 为什么现在进入 RFC

该变化会同时影响：

- `Justfile` 与 `scripts/xtask` 的命令、config types、tasks 和 artifact export；
- `kconfig` / `.defconfig`、platform、architecture、rootfs 与 app 配置边界；
- kernel 生成输入、root mount specification、DTB delivery 和启动 entry；
- end-to-end wrappers、tracked schemas/examples、开发文档与构建技能；
- 平台内核产物、外部输入和失败诊断。

这些 surface 必须按共同 target 和迁移 gate 变化，不能拆成一组互不知情的小修。

## 目标

- 为 architecture/compiler target、platform、system target、KernelConfig、rootfs/app
  build task、build preset、resolved build 和 invocation 建立唯一 owner。
- 让 `Platform` 只描述具体 physical/virtual guest machine contract，不拥有产品用途、
  root role、initial app、`CargoProfile` 或 host artifact path；Platform 同时拥有其 boot ABI
  要求的 kernel output format，包括可选 U-Boot legacy-image 参数。
- 让 `SystemTarget` 描述系统产品的 boot/deploy contract：platform reference、root
  mount、Boot Protocol entry source 和 kernel capability requirements。
- 让 `KernelConfig` 只描述编入 kernel 的 capability、policy 和容量，不再拥有
  platform、`CargoProfile` 或 disassembly。
- 让 `BuildPreset` 只命名一组可复用选择，不复制或覆写 target、platform、
  KernelConfig、app/rootfs task 或 invocation 语义。
- 要求每个system action显式提供一个named preset或完整low-level tuple；不读取developer-local或
  repository-default selection，不允许preset携带presentation defaults。
- 要求rootfs base type、QEMU CPU和format scope显式；folder rootfs容量统一自动计算。QEMU BIOS
  仍是有意义的optional capability，省略表示不传`-bios`。
- 将 Anemone Boot Protocol 从固定的 `/.anemone/init` entry 扩展为 typed
  `InitialProgramSource`；`RootfsEntry` 与 `EmbeddedApp` 最终都解析为稳定 VFS path，
  并统一进入普通 `kernel_execve()`。
- 为每次实际 build/QEMU execution 派生不可手写、不可变的 `ResolvedSystemBuild`，使 action
  不在执行中途重读可变选择。
- 让 `build` 不读取、生成或要求 rootfs、测试盘、network backend 等 runtime artifact，
  但生成 selected Platform 要求的全部 kernel artifacts，包括必要的 U-Boot post-link image。
- 保持 app/rootfs 的 task-specific manifest 与 action，不为跨 action 固定路径消费建立 typed
  artifact graph、publication protocol 或 freshness proof；所需命令顺序由 recipe 注释、文档
  或 wrapper 明确表达。
- 为 app build 增加 closed `Source` driver，使已有 binary、shebang script 或其它普通文件不经
  编译即可进入现有 artifact 校验与导出管线；no-op 只表示不执行 build command，不跳过输入校验、
  artifact export 或下游 consumer 验证。
- 将 QEMU virtual machine contract、tracked argv template 和一次 invocation 的 host path
  分开；用 QEMU-local `[[qemu.bind]]` 表达受控 argv 空位，不建立跨 action 的 generic
  binding 或 launcher manifest。
- 为每个platform明确DT authority与DTB delivery。Physical Platform在没有可执行provider输入时保留
  committed DTS；QEMU Platform不提交DTS，firmware delivery直接消费runtime FDT，embedded delivery
  由normal build从selected QEMU provider物化build-local DTB。
- 要求QEMU bind只参数化DT-neutral runtime attachment；build-time DT物化不读取、展开或伪造bind value。
- 保持 `Justfile` / `scripts/xtask` 为构建编排 owner，并在接口 cutover 时同步 tracked
  schema、examples、wrappers、build docs 与 `anemone-build-system` skill。
- 保持 pretest、LA64 和 physical-board 路径可迁移、可回归；VisionFive `[uboot]` 继续由
  Platform 拥有，迁移只 Preserve 其 header、load/entry、产物名和板级启动行为。

## 非目标

- 不在本 RFC 中实现 final runner、评分、case selection、marker parser 或赛方镜像兼容。
- 不把 final harness、pretest 或任何 workload mode 做成 Kconfig feature/enum。
- 不在 Draft review、Stage 1 Ready 解析与后续独立授权完成前建立 implementation
  transaction 或开始代码实现。
- 不借本 RFC 修改仓库 RFC workflow；本次 public promotion 只同步本 RFC、最小 current
  baseline 与公共导航。
- 不增加 `package` action、package backend/config、`[[outputs]]`、target logical-output graph
  或通用 artifact transformation framework；提交导出和产品装配留给具体 adopter/workflow。
- 不把 `Source` driver 扩展成任意 command/shell driver、下载器、格式转换器或 artifact-type
  detector；它不执行脚本、不推断 binary architecture/shebang compatibility，也不自动修正 mode。
- 不建立跨 action artifact publication/freshness protocol、跨 resolution artifact cache、
  per-artifact semantic-input closure、content-addressed build graph 或 host-tool fingerprint
  系统。固定路径的跨 action 消费允许依赖明确命令顺序。
- 不建立HostEnvironment resolver、`--tool` override、local executable binding、host-tool版本协商
  或capability discovery；xtask只按仓库固定程序名调用并依赖开发者`PATH`。
- 不增加generic inspect命令或human/JSON resolution view；实际action只打印必要的
  selection source、canonical references与resolved snapshot摘要。
- 不为 QEMU runtime path 建立 generic external-role/slot/disk binding hierarchy；第一版只
  支持 QEMU-local argv template。
- 不建立 generic launcher manifest，也不为 physical platform 制造伪 QEMU provider。
- 不把 rootfs recipe 搬进 Kconfig，不让 platform 拥有 root role 或 initial app。
- 不把 `ResolvedSystemBuild` 手写或提交成第六类 canonical config。
- 不承诺自动感知 compiler/linker/sysroot 等被底层增量构建隐式消费的 host build
  environment 变化；改变未跟踪的隐式环境输入后必须执行相应 clean。`dtc`/`mkimage`
  由对应 build stage 直接调用，不为它们建立 fingerprint。
- 不在普通kernel build中读取外部镜像、runtime bind或backend。只有selected QEMU Platform声明
  `embedded` DTB delivery时，build可以用provider-owned topology snapshot执行`dumpdtb`；该例外不得扩展成
  普通QEMU execution、source-tree output或用户可选DT maintenance命令。
- 不把 U-Boot image header、load/entry、output naming 迁入 SystemTarget 或新的 package
  abstraction，也不借本 RFC 改变 physical-board deployment contract。
- 不借构建系统重构改变与本 target 无关的 kernel runtime ABI、测试评分规则或 RFC
  治理政策。

## 文档地图

当前 R4 accepted target：

- [目标与不变量](./invariants.md)：target rules、唯一 owner、resolved snapshot、action scope、
  platform kernel outputs、DT 与 workflow 同步边界。
- [迁移实施计划](./implementation.md)：rolling stage envelope、feedback/preflight gate、
  validation floor、停止条件与回写路径；Stage 1-4历史定义及resolved manifest的唯一权威。
- 本文：范围、方案、接受边界、备选与风险。

Review 状态：

- [Tracking Issues](./tracking-issues.md)：已确认问题的分流与 neutralize 依据；当前无 live
  design blocker。

背景材料：

- [背景材料索引](./backgrounds/index.md)
- [RFC 前定位共识](./backgrounds/positioning.md)
- [Final Harness 调查记录](./backgrounds/final-harness-investigation-20260722.md)

`implementation.md` 只描述后续实施边界。Public promotion与初始Implementation Resolution Gate
已完成；Stage 1 Ready definition/manifest已冻结。R0 acceptance、transaction creation 与
Stage 1 activation 已在 2026-07-23 独立闭合；Checkpoint 1A-1D已依次独立关闭，Stage 1 Closed。
后续独立resolution gate已把Stage 2解析为Ready；2A-2D随后分别独立激活并关闭。Stage 3现已由
后续独立resolution gate解析并激活；R1 DT authority renegotiation在Stage 3停止后由用户接受。Stage 3与
R2 feedback correction独立关闭后，Stage 4已解析为单一Checkpoint 4A，并由本轮独立授权完成和关闭；
Stage 5与单一Checkpoint 5A现已独立关闭；Stage 6与单一Checkpoint 6A也已完成最终收口并关闭。

## 术语与 owner

| 概念 | 唯一职责 | 明确不拥有 |
| --- | --- | --- |
| Architecture / compiler target | ISA、ABI、target triple、toolchain contract | machine、root、initial app、产品用途 |
| Platform | guest-visible physical/virtual machine、boot ABI、device topology、DT contract、kernel output format 与 QEMU argv template | root role、workload、host bind value、`CargoProfile` |
| System target | 产品 boot/deploy contract、root/entry selection、required capabilities | kernel 参数、machine fact、kernel image format、一次 build 的完整身份 |
| KernelConfig | kernel capability、内部 policy、容量 | platform、rootfs、`CargoProfile`、disassembly |
| App/rootfs task | task-specific manifest、driver、输入、构建或source采纳、artifact导出与rootfs组合 | boot policy、runtime registry、跨 action freshness proof |
| Platform kernel output | boot ABI 要求的 kernel output format 与参数 | root/entry policy、产品装配、host destination |
| Build preset | target + KernelConfig + kernel-only `CargoProfile` 的具名选择 | target/KConfig overlay、presentation defaults、app task profile、QEMU bind value |
| Resolved system build | canonical inputs 派生的不可变 selection/config snapshot | 手写配置、artifact cache key、QEMU bind value |
| QEMU bind declaration | bind name与固定 argv token template | host path、artifact type、跨 action role semantics |
| Action resolution | 某次 build/QEMU execution 的所需配置与 host presentation | 改写已经解析的 target/platform/KConfig truth |

引用不是 owner 重叠：target 可以声明 required kernel capabilities，preset 可以选择一份
具体 KernelConfig，resolver 负责验证满足关系；target 可以引用 platform，但不得复制
machine facts。

## 方案

### Platform 与 provider

Platform 描述 kernel 实际面对的机器。它拥有 architecture/firmware environment、
memory/CPU/device topology、boot ABI、DT authority 与 delivery。

QEMU-backed virtual platform 可以在同一配置边界拥有 QEMU provider section。Provider
保存物化 machine contract 所需的固定 argv。若该Platform使用`embedded` DTB delivery，normal build
可以从已经解析的provider snapshot生成build-local DTB；若使用`firmware` delivery，runtime QEMU/firmware
直接提供FDT，build不生成DTB。两种路径都不提交QEMU-derived DTS，也不建立用户可调用的DT
refresh/check maintenance surface。

Host executable 不进入 public config，也不建立 environment resolver、override、版本协商或
capability discovery。Xtask 按 action 与 architecture 直接调用仓库固定的程序名，例如
`qemu-system-riscv64`、`qemu-system-loongarch64`、`dtc` 或 `mkimage`，由进程环境的 `PATH`
完成普通 executable lookup；命令不存在或执行失败时直接返回对应 action error。需要自定义
binary 的开发者自行通过 `PATH` 提供同名命令，本 RFC 不提供额外 binding surface。

固定 QEMU argv 中需要调用者提供 host path 的位置由同一 section 内的 `[[qemu.bind]]`
声明。它是 QEMU-local argv template，不是 SystemTarget product role、provider-neutral
input schema 或 generic workflow binding。第一版 schema 固定为：

```toml
[[qemu.bind]]
name = "disk-x0"
template = [
    "-drive",
    "file={{}},format=raw,if=none,id=x0",
    "-device",
    "virtio-blk-device,drive=x0,bus=virtio-mmio-bus.0",
]
```

`template` 是 argv token array，不是 shell command；每个 token独立传给
`std::process::Command::arg()`，不得按空格二次切分。`{{}}` 是唯一占位符语法，
同一 template 可以出现多次，并将每处替换为同一个 bind value；至少出现一次，否则该项
应直接进入固定 `args`。第一版每个声明的 bind 都是 required host path，不提供 optional、
default、source kind、block slot 或 disk subtype。Name 在同一 QEMU section 内唯一，bind
declaration 的配置顺序决定展开顺序，CLI 顺序不得改变 argv topology。

Name只说明QEMU argv空位及其guest-visible attachment，不编码`rootfs`、
`test-data`、`competition-root`等SystemTarget/workload role。开发者或wrapper负责按selected
platform的tracked config提供正确path；binding layer不再为这层人工选择建立第二份语义模型。

普通 `qemu` execution 在启动 QEMU 前拒绝未知、重复、缺失、空值或不存在的 path。替换不得经过 shell；
QEMU keyval 中含逗号的 path 第一版直接拒绝，不能让 bind value 改写模板固定的 format、
drive id、device 或 bus。Tracked config 只保存 name 和 template，不保存开发者
home path、worktree-local image path 或机器专属探测结果。

会改变 guest-visible CPU/device/firmware contract 的 QEMU option 不是 presentation，
必须进入 platform/provider identity，必要时形成 platform variant。Physical board 与
模拟它的 QEMU machine 始终是不同 platform identity。

Platform 还拥有其 boot ABI 要求的 kernel output format。VisionFive 的 `[uboot]` 明确属于
这一层：它声明正常 `build` 在导出 kernel ELF 后还必须运行 `objcopy + mkimage`，生成既有
legacy image。该转换可以在代码中作为独立 post-link stage 实现，但不是独立 `package` action、
SystemTarget output 或可插拔 backend。Header、load/entry、name 与 filename 在迁移中保持现有
行为；其中可以安全从 architecture、ELF 或其它 Platform 单一真相源推导的字段，可在对应
implementation preflight 中收缩，但不得复制到另一层。

### System target

System target 引用一个 platform，并拥有：

- root mount specification 与 root source selection；
- Anemone Boot Protocol entry source 与 initial app artifact reference；
- required kernel capabilities。

Target 不选择具体 KernelConfig，也不拥有 `CargoProfile`。它的 capability requirements
由 resolver 对所选 KernelConfig 验证。Kernel image format 属于 referenced Platform 的
boot ABI；比赛提交名、导出目录或完整介质装配属于 adopter/workflow，不进入 SystemTarget。
这里的 deploy contract 只表示系统引用的 root/entry source 与 required capabilities，不表示
构建系统负责完整打包。是否额外生成 disassembly 或如何显示日志仍不是 target semantics。

### Build preset

Build preset 是用户入口，不是产品或配置真相源。其 semantic selection 只包含：

- 一个 system target reference；
- 一个 KernelConfig reference；
- 一个 `CargoProfile`。

`CargoProfile` 只选择 kernel Cargo build 使用的 profile，并作为 kernel build input。
它不传播到 app/rootfs task，也不覆写 app manifest 中由该 task build contract 拥有的
Cargo 参数或 profile。App build 若需要 debug/release 等差异，仍由自己的 manifest/driver
输入表达。

Preset不提供action presentation defaults。`disasm`、debug、本地日志/输出视图等必须由
调用者在本次action中显式给出；若某个选项会改变kernel bytes、guest contract或build task
调用参数，它必须进入对应canonical owner与`ResolvedSystemBuild` snapshot，不能伪装成presentation。

Preset 不保存 QEMU bind value、runtime image path、debug switch 或 target 字段 overlay。
具体 QEMU host path 只存在于本次普通 `qemu` invocation。

RFC schema 必须展示一对多关系，例如同一 `pretest-rv64` target 可以被 release/dev
preset 复用；target 与 preset 不再默认使用相同名称掩盖这两个维度。

### CLI 显式输入与 action interface

`Justfile` 是用户和 agent 的稳定 common-flow 门面，recipe 只向 `scripts/xtask` 转发参数；
xtask 是 selection、resolution、action execution 和 structured diagnostics 的唯一 typed
owner。不得增加第三套 build CLI。Architecture-specific wrapper 只保留外部
master image 安全复制、日志、sudo/host prerequisite 与 end-to-end 收尾，不解析 semantic
config，也不自行拼 platform/QEMU command。

所有需要system selection的action共享同一显式输入语法。一次调用必须且只能使用以下两种
完整来源之一：

```text
explicit preset:
    --preset <preset-ref>

explicit low-level selection:
    --target <target-ref> --kernel-config <kernel-config-ref> --profile <profile>
```

两个形状互斥。Low-level selection缺少任一字段、同时混用preset与tuple或完全省略输入时直接
失败，不能从local state、tracked default、其它CLI option或presentation flag补齐。仓库不保存
developer-local或repository-default selection。每个action在开始时报告显式输入种类、canonical
references与resolved snapshot；随后只
使用一次解析形成的不可变 `ResolvedSystemBuild`，不能中途重读 selection/config。

第一版 stable common-flow family 为：

```text
just build     (--preset <preset> | --target <target> --kernel-config <path> --profile <profile>)
just qemu      (--preset <preset> | --target <target> --kernel-config <path> --profile <profile>) --bind <qemu-bind>=<host-path>...
just qemu      (--preset <preset> | --target <target> --kernel-config <path> --profile <profile>) --show-bindings
just fmt       <all|kernel|app> [--check]
just clean     ...
```

这里的 `build` 生成 selected Platform 的 kernel artifacts：至少导出 kernel ELF，并在 Platform
声明 `[uboot]` 时生成对应 legacy image。它不读取或要求 rootfs、test disk、network backend。
普通 `qemu` 从 target 引用的 platform 取得 QEMU provider，并要求调用者填满该 platform
QEMU section 声明的全部 bind。Physical
platform 没有 QEMU provider 时，普通 `qemu` 明确报告 action 不受支持，不为统一外观建立
generic launcher。

`--bind name=path` 只填充 selected platform 已声明的 QEMU bind；它不选择 product role、
device slot、format 或 source，也不形成跨 action binding API。Kernel、rootfs、test image等
动态 QEMU host path 若需要由调用者提供，都使用同一 `[[qemu.bind]]` 机制；普通 `qemu` 不再
保留特殊 `--image` path。第一版明确接受完全人工映射：调用者根据 bind name 与 argv template
选择 path，resolver/QEMU action 不证明该 path 对应 SystemTarget root selection、
architecture、format 或先前 build/rootfs action result。Path 通过机械校验但内容选错时，可以在
QEMU/guest boot 或 wrapper 验证中失败；这不是本阶段 build model 承诺提前拒绝的 mismatch。
`--show-bindings` 在解析显式输入/platform后打印每个bind的name与argv template并退出，
不启动QEMU，也不要求先提供bind value。
第一版不提供raw `--qemu-arg`；用户只能填充tracked template中的`{{}}`，不能增加新argv
token或改写template固定部分。Debug和console继续是明确的action option；host tool只按仓库固定
程序名从开发者`PATH`调用，不伪装成QEMU bind。

QEMU DT没有独立用户命令。Build-time provider materializer只消费本次
`ResolvedSystemBuild`中的selected Platform、architecture与QEMU provider topology snapshot：只使用
machine、CPU、SMP、memory和optional BIOS执行`dumpdtb`，不解析另一份Platform reference，也不读取
ordinary `qemu.args`、SystemTarget runtime role、rootfs/test disk或execution bind map。输出先写入
build-local temporary file，验证provider成功产生DTB后原子发布；失败不得保留可被kernel compile误认的
stale/partial output。

Ordinary `qemu.args`与`[[qemu.bind]]`的长期边界因此收紧为DT-neutral invocation/attachment。Normal build
不复用ordinary args，也不为bind提供占位文件、dummy backend或类型推断；若新增参数会改变provider DT，
它已经改变Platform topology，必须停止配置变更并回到Platform review，把guest-visible fact提升为显式
topology字段并单独解析其物化方式。

Exact short flag、Clap struct 分拆和 local file 路径可以在 implementation preflight 中选择；
最小 canonical object schema 与 reference identity 必须在 Stage 1 冻结，不能留到 Stage 2
才决定；
上述 command family、selection precedence、agent 显式选择、action 语义、QEMU bind 边界和
observability 是 target contract，不能在实现时改变。

### KernelConfig、app/rootfs task 与 platform kernel output

KernelConfig 只包含 kernel feature、policy 和容量参数。Platform、`CargoProfile` 和
disassembly 从现有 `[build]` 分离；具名 KernelConfig 可以被多个 compatible target
复用。`CargoProfile` 是独立于 KernelConfig 的 kernel build selection；app task 的 Cargo
profile 继续由 app manifest/driver 拥有。

App/rootfs config 继续是各自 task 的直接 contract：

- Cargo app driver 构建 architecture-specific artifact；
- Source app driver 是 build-command no-op：`driver = "source"` 不启动子进程，而是使用与其它
  driver 相同的 workdir、artifact path 展开、普通文件校验和 export contract，直接采纳已经存在的
  binary、shebang script 或其它普通文件；
- rootfs recipe 组合 base、目录、文件和 app artifact，输出 filesystem artifact。

`Source` 不意味着跳过 app task。缺失路径、目录或其它非普通文件必须在 export 前失败；因为没有
可接收参数的 build command，调用者提供额外 driver args 时必须拒绝，不能静默忽略。Source 不读取
artifact 内容来推断格式或 architecture，不调用 `/bin/sh`，不做 chmod、rename、fetch 或转换，也不
建立第二套 source-path schema。已有 artifact path expansion 与公共 export 逻辑仍是唯一实现面。

Binary 或 script 能否作为 `EmbeddedApp` / rootfs executable 使用是下游 contract：source owner
必须提供适合目标 architecture 和 consumer 的 bytes/mode；直接 exec 的 shell script 必须具有有效
shebang，且解释器必须由最终 VFS namespace 提供。App build 只证明声明路径被机械采纳和导出，不把
“文件存在”误述为普通 exec/binfmt 或 runtime compatibility proof。

这些 task 不决定 boot policy。需要独立 app/rootfs action 的流程由调用者显式运行对应命令，
或由具体 wrapper 按固定顺序组合；本 RFC 不建立跨 action typed dependency graph、publication protocol 或自动
freshness check。VisionFive rootfs recipe 可以继续从 `build/anemoneImage-rv64` 复制文件，
并在 recipe 注释或相邻文档中明确要求先成功运行同一 selection 的 `just build`。这项约束用于
说明正确工作流，不要求 xtask 证明文件来自当前 invocation。

Platform `[uboot]` 是 `build` 的 kernel-output contract。内部实现先链接/导出 ELF，再执行
U-Boot post-link；只要 Platform 声明该输出，用户不需要也不能通过第二个 package 命令选择它。
`EmbeddedApp` 后续若需要 build action 内部调用 app driver，Stage 5 只解析这一条 Boot Protocol
所需的窄依赖，不据此恢复通用 artifact graph。

### Anemone Boot Protocol

当前 effective baseline 是 kernel 挂载 rootfs 后读取 `/.anemone/init`，取得一个绝对
可执行路径，再通过普通 `kernel_execve()` 启动初始用户进程。该规则已经存在于 live
kernel/rootfs owner，但尚未提取为 `docs/src/contracts/` current contract；后续不能把
“首次写 contract 文档”误分类为全新 `Introduce`。

本 RFC target 将该 baseline Refine 为有限的 typed `InitialProgramSource`：

```text
Anemone Boot Protocol
└── InitialProgramSource
    ├── RootfsEntry
    │   └── 从 rootfs metadata 解析 executable path
    └── EmbeddedApp
        └── 从 build-time app artifact 物化 executable path
```

SystemTarget 拥有 source selection 与 app reference；build resolver/materializer 负责把
canonical target 输入转成有限 kernel boot specification；kernel Boot Protocol owner只
消费该规格，不解析 target/preset/app manifest，也不识别 final harness 等 workload。

两种 source 最终都必须形成稳定、可重新打开的 VFS executable path、argv 与 envp，
并进入普通 `kernel_execve()`、ELF/shebang dispatch 与 mandatory user-entry contract。
Embedded script 的 path 在 interpreter 重新打开期间必须持续有效，不能只把临时 bytes
交给首次 binfmt probe，也不能建立第二套“执行内嵌 bytes”路径。

该协议是本 RFC 当前唯一明确计划提升为长期 current contract 的 target invariant。
Root mount、resolved snapshot、platform kernel output、DT build/delivery 和 repository workflow
仍是本 RFC 的实现约束；只要它们不改变额外的 kernel/runtime shared semantics，就不
进入 `Contract Impact`。若后续 target review 发现 runtime FDT 接受或 root-mount ABI
也发生语义变化，必须显式回到本节和 `Contract Impact` 扩大范围。

### Resolved system build 与 action closure

Resolver 从 canonical inputs 派生 `ResolvedSystemBuild`，至少固定：

- system target、platform 与 architecture/compiler target identity；
- 精确 KernelConfig 与 kernel-only `CargoProfile`；
- target 引用的 initial app/root source identity；
- action 解析需要的其它 canonical references 与 requirements。

该结果不可由用户手写，也不能被后续 action 通过重读可变选择改变。它是本次 action 的
selection/config snapshot，不是 artifact cache key，也不用于证明跨 action 产物 freshness。

每个 action 再从 resolution 解析自己的输入范围：

```text
build
    platform + KernelConfig + boot/root specification
    kernel ELF + platform-required post-link outputs
    不要求 rootfs/runtime disk 存在

rootfs materialization
    rootfs recipe + task-specific app/file inputs
    可以按文档约定消费先前 build 的固定输出路径

QEMU execution
    resolved platform QEMU config + 本次完整 QEMU bind map + host tool
```

QEMU bind path、debug switch 与 console presentation 属于
action-scoped input，不进入 resolved selection。QEMU binding layer只验证本次map完整
匹配selected platform的声明并满足host path约束，不解释product role、architecture、slot或
format，也不验证先前build/rootfs action的result或SystemTarget root selection。普通QEMU
bind不会把人工path映射升级为artifact consumer关系，也不能从template内容反向猜测artifact
类型。跨action的repository output同样不获得隐式freshness证明；调用者或wrapper负责执行并
记录所需命令顺序。

### Device tree

Platform必须分别声明runtime delivery、DT authority与provider，并为manifest、可选committed DTS、
provider output和runtime FDT中的每项machine fact指定唯一规范owner。Delivery说明DTB如何进入内核；
authority说明machine fact由normative source还是实际provider拥有。Committed source不是所有Platform的
强制外观，只在没有可执行provider输入、需要reviewable长期source/baseline的physical Platform保留。
存在`[qemu]`即把该identity分类为QEMU Platform，并要求`provider = "qemu"`且不允许committed source；
physical source/baseline contract必须没有`[qemu]`，模拟physical board的QEMU model使用另一Platform identity。
R4A保持kernel delivery ABI只读，因此supported matrix同时要求`riscv64 = firmware`、
`loongarch64 = embedded`；反向组合不是可配置的future capability，必须在action side effect前拒绝。

- QEMU + `firmware`：QEMU machine model拥有machine fact，runtime FDT由provider/firmware直接交给kernel；
  manifest不引用committed DTS，normal build删除stale build-local DTB后继续；
- QEMU + `embedded`：QEMU machine model拥有machine fact，normal build从selected provider topology snapshot执行
  `dumpdtb`，验证并原子发布build-local DTB供kernel嵌入；manifest不引用committed DTS；
- physical + `embedded`：committed normative DTS继续由normal build通过`dtc`生成build-local DTB；
- physical + `firmware`：committed provider-derived DTS可作为人工review的conformance baseline；capture
  provenance、允许的runtime差异和复核责任保存在baseline相邻说明与RFC/transaction证据中，不建立没有
  action consumer的typed字段；normal build不为firmware delivery生成DTB；
- 生成DTB只位于build output且不提交。QEMU provider materialization不读取rootfs、测试盘、network
  backend或任何bind value，physical DTS也不会被QEMU output自动改写。

R4A接受QEMU raw provider DTB中的`/chosen/rng-seed`。Embedded LA64 build因此不承诺DTB或kernel artifact
byte-for-byte reproducibility，也不承诺该build-time seed在每次boot刷新；当前kernel不消费该属性，这两项
不是R4A acceptance。只有后续出现真实consumer、安全要求或reproducible-build要求时才重新解析。

### Repository workflow 同步

本 RFC 改变的是 build workflow，而不是 RFC governance。后续每个 build-interface
cutover 必须在同一 gate 同步受影响的 durable surfaces：

- `Justfile` 与 `scripts/xtask` 的 live CLI/help/config/task owner；
- tracked defaults、schemas、examples 与配置迁移说明；
- architecture-specific end-to-end wrappers 和用户可见输出约定；
- build/config 文档；
- `.agents/skills/anemone-build-system/SKILL.md` 及其必要 references。

技能和文档保存稳定 owner、路由与验证规则，不复制容易漂移的当前 platform 值或完整
CLI option table。若 live help、config deserialization、task code、schema/example 或 prose
冲突，以 live config/task owner 为事实源，并在对应 cutover 修复其余 surface。

本 RFC 不默认修改 `anemone-rfc-doc-workflow`、RFC template 或公共治理规则；只有未来
target review 证明治理 contract 本身需要变化时，才另行提出 repo-wide workflow delta。

### Current-to-target CLI delta

以下表只冻结迁移方向，不是逐文件 write set，也不宣称 target CLI 已经实现：

| Current surface | Target disposition |
| --- | --- |
| `just conf switch <platform>` 修改 root `kconfig` | 删除；system action只接受显式preset或完整low-level tuple |
| `conf list` 只枚举 platform | 保留现有 platform discovery；第一版不增加跨对象 inspect framework |
| `just build` / `xtask build -k <file>` | 使用共享resolver并要求显式完整输入；生成selected Platform的kernel artifacts，包括其声明的U-Boot post-link output，但不构建rootfs/runtime backend |
| `just xtask qemu --platform ... --image ...` | 普通execution由`just qemu <explicit-input> --bind name=path`取代；显式输入确定platform，所有动态QEMU path由该platform的`[[qemu.bind]]`声明，不保留特殊`--image` |
| QEMU DTS baseline与`just qemu dt refresh` maintenance surface | 删除；QEMU firmware delivery直接使用runtime FDT，QEMU embedded delivery由normal build从selected provider自动生成build-local DTB，且不读取runtime bind/backend |
| Platform `[qemu].qemu = "qemu-system-*"` | 从 public platform schema 删除；xtask按action/architecture直接调用仓库固定程序名，并依赖开发者`PATH`完成普通查找 |
| 无 resolution read-only view | 不增加inspect命令；实际action在开始时打印selection source、canonical refs与resolved snapshot摘要 |
| `app` / `rootfs` 直接 task | 保留直接 action 与各自 manifest；app driver收口为closed Cargo/Source variants，Source不执行command但复用公共artifact校验/export；跨 action 固定路径依赖由注释、文档或 wrapper 声明顺序，不建立通用 artifact graph |
| `just defconfig` 同时重置 `[build]` 与 kernel参数 | 保留 reset 能力，但只生成/重置 local KernelConfig；platform/`CargoProfile`/disasm由其它 owner选择 |
| `gendisk` 覆盖固定 `disk.img` | 从 common surface 删除；需要的 filesystem/disk preparation由具体 workflow 拥有 |
| `clean` / `mrproper` / `xtask-clean` 重叠 | 收敛为单一ordinary clean；不删除用户KernelConfig |
| bare `just fmt` | 删除隐式all scope；要求显式`all`、`kernel`或app name，`all`覆盖kernel、xtask和全部apps |
| rootfs省略`fs.type`或配置容量 | `fs.type`必填；folder容量统一由`virt-make-fs`自动计算，不暴露容量策略字段 |
| QEMU省略CPU依赖provider default | 每个QEMU Platform显式固定CPU；BIOS保持optional，省略只表示不发出`-bios` |
| pretest wrapper 解析 `kconfig`、切 platform、寻找/链接 artifact、直接调 QEMU | wrapper显式选择preset，复制只读master后按`qemu --show-bindings`/tracked config提供QEMU bind，并保留日志与host prerequisite；不再拼raw QEMU argv或制造根目录固定文件名 |

### Final harness 作为后续 adopter

Final harness 可以在本 RFC 模型落地后，通过一个 system target、compatible
KernelConfig 与 Source 或 Cargo app task 接入；提交文件重命名或装配由 adopter
workflow 负责。本地QEMU验证所需的
competition image path由对应platform的QEMU bind提供。该小迭代负责runner行为、评分和
镜像兼容，并复用这里建立的target/preset/resolution contract；它不得把final-specific
binding提升为通用build model或跨provider schema。

## 接受边界

R0 acceptance gate 已于 2026-07-23 核对并确认：

- [目标与不变量](./invariants.md) 中的 owner、resolved snapshot、action scope、platform kernel output、
  DT authority 和 workflow sync rules 形成无双重真相源的闭包；
- 已确认的 target-level Keter 已 neutralize，修复已折回本文或 invariants；
- existing `/.anemone/init -> kernel_execve()` baseline 已提取为最小
  [`BOOT-PROTOCOL-001`](../../contracts/task/boot-protocol.md#boot-protocol-001--typed-initial-program-source统一收口到普通-vfs-exec) current contract；其 Refine target、唯一局部 owner、ordinary VFS
  exec、稳定 reopen lifetime 和 cleanup obligation 已闭合；
- target/preset/platform-output/presentation/invocation 的边界能由最小 schema 草图表达，并包含
  至少一个 target 被多个 preset 复用的实例；
- `ResolvedSystemBuild` snapshot、platform kernel output 与 QEMU invocation bind map 的分离
  已经明确；跨 action 固定路径依赖不被误述为 freshness guarantee；
- QEMU bind 明确保持完全人工映射；普通 QEMU action 只验证 declaration/map/path 的机械
  正确性，不承诺证明 bind value 满足 SystemTarget root selection 或先前 action result；
- U-Boot kernel output 由 Platform 拥有并作为 `build` 的 post-link stage 保持 current
  behavior；任何字段收缩必须先证明未改变 boot ABI 或 physical-board workflow；
- DT authority/delivery 目标已固定；每个平台的具体 baseline 在该 platform 进入迁移前
  通过滚动 gate 分类；
- CLI只接受显式完整输入，不保存local/repository-default selection；
- final harness 已明确降为后续 adopter，不进入首个 implementation target；
- `Source` app driver 的 no-command / common-export 边界已经闭合：它允许直接采纳已有 binary、
  shebang script 或其它普通文件，但不执行 shell、不解释或转换内容，也不声称证明 runtime
  compatibility；对应 build/export 与 Boot Protocol 验证分别进入 Stage 4 和 Stage 5。
- [迁移实施计划](./implementation.md) 已通过初始 `Implementation Resolution Gate` 把 Stage 1
  的交付、实现路线或 probe、审计、可观测性、验证、停止/退出条件、contract cutover 与
  `Resolved Write Set Manifest` 完整解析为 Ready，Stage 1现已完成并关闭；后续独立resolution gate
  已依次把Stage 2至Stage 6完整解析为Ready，2A-2D、3A、4A、5A与6A随后分别独立关闭。

[迁移实施计划](./implementation.md) 已记录允许带入实现的不确定性。Public promotion、首个
Implementation Resolution Gate、R0 acceptance、transaction creation 与 Stage 1 activation 已按独立
gate 完成；Checkpoint 1A-1D与2A-2D已独立关闭。Stage 3 implementation反馈触发的DT authority renegotiation
已由用户接受为R1，Checkpoint 3A与Stage 3随后关闭；关闭后的metadata反馈已由用户接受为R2并完成correction；
后续独立gate解析并关闭了Stage 4/Checkpoint 4A、Stage 5/Checkpoint 5A与最终Stage 6/Checkpoint 6A。
R3反馈已由用户接受：删除implicit local/default selection与preset presentation defaults，并把
rootfs type、QEMU CPU和format scope收口为显式输入；folder自动容量与optional BIOS是唯一记录的
省略语义。自然可空collection、optional Platform capability与opt-in action flag的省略只表示“没有
该项”，不得选择另一套target/config/policy。

R4反馈进一步确认：QEMU machine model已经是可执行machine-fact authority，提交由它导出的DTS并维护
refresh/check只形成第二份provider镜像。QEMU Platform因此不再提交DTS；RV64类firmware delivery直接
接受runtime FDT，LA64类embedded delivery由normal build自动物化build-local DTB。QEMU bind必须
DT-neutral，build不提供占位符；QEMU DT maintenance CLI整体删除。Physical Platform在没有等价可执行
provider输入时继续提交稳定DTS source/baseline。R4A完成该DT cutover；hardware-semantic SMP Platform
与business-semantic target/preset映射由R5移出本RFC，作为模型之上的独立配置应用维护。

若后续feedback需要
改变target invariant、owner、ABI、
`Contract Impact`或acceptance boundary，当前gate必须停止并回到RFC review。

## 备选方案

### 合并 system target 与 build preset

拒绝。Target 是跨 `CargoProfile` 和 KernelConfig 复用的产品 contract；preset 是具名
选择。合并后会迫使 target 拥有 kernel 参数或构建展示，并恢复当前 config 混层。

### 只保留显式 CLI 参数，不提供 preset

保留为低层接口候选，但不作为唯一用户入口。完全依赖每次手写 target/KConfig/`CargoProfile`
会重复当前 mutable `kconfig` 选择问题，也不利于受支持组合的回归矩阵。Preset 必须是
引用组合，而非 overlay。

### 继续复制 pretest/final platform

拒绝。产品用途、root role 与 workload 不属于 machine identity；复制会让 machine
facts、DT 与 provider drift。

### 继续提交 QEMU-derived DTS 并保留 refresh/check

拒绝。QEMU DTS是provider输出的reviewable镜像，不是独立machine-fact authority；SMP等拓扑variant会把
同一provider truth复制成多份baseline，并要求仓库维护没有runtime consumer的diff/canonicalization
surface。Firmware delivery应使用runtime FDT，embedded delivery应在build中从selected provider自动
物化。可复现QEMU环境由外部toolchain约束解决，不通过提交provider snapshot伪装版本固定。

### 把产品模式放入 Kconfig

拒绝。Kconfig 可以拥有 EmbeddedApp support 等 kernel capability，但不能拥有
final/pretest、root artifact 或 initial app selection。

### 建立 generic launcher manifest

拒绝。QEMU provider 是 virtual platform 的 typed capability；physical platform 不应为
统一外观获得伪 launcher。一次性 host presentation 留在 invocation。

### 为 runtime path 建立 generic artifact binding

拒绝。当前 concrete consumer 只有QEMU。`[[qemu.bind]]`只参数化受版本控制的QEMU argv，
不建立external role、block slot、disk subtype或跨deploy/QEMU execution复用的binding layer。
当前阶段接受调用者完全人工完成bind name到host path的映射，也接受语义选错的有效path只能在
QEMU/guest/wrapper验证中暴露；不为了提前证明SystemTarget role fulfillment引入typed
attachment handoff。未来只有出现第二个真实consumer、人工映射形成不可接受的实际失败证据，
或用户明确扩大保证边界后，才重新评估提取共享抽象。

### 建立独立 package / U-Boot action

拒绝。当前真实需求只是 VisionFive Platform 要求 `build` 在 kernel ELF 后生成一个确定性的
U-Boot legacy image。把 `objcopy + mkimage` 提升为 `package` CLI、backend 配置、target output
graph 或 `[[outputs]]`，会增加用户编排和跨 action freshness 问题，却没有第二种格式、重复
repackage 或 ELF-only 高频工作流来证明抽象价值。内部 post-link stage 保持可分离；只有未来
出现同一 ELF 反复转换、多种可选格式、显著昂贵的可选转换或独立发布需求时，才重新评估
用户可见的转换命令。

### 一次性重写全部 build system

拒绝。后续 implementation 应按 owner-local gate 迁移并保持 pretest/physical paths
可验证；本 target 不预先冻结所有阶段的文件清单。

## 风险

- 配置分层过细可能制造只转发引用的 ceremony；schema 必须由三个现有场景和真实
  复用关系证明，而不是追求概念齐全。
- 固定路径跨 action 消费可能接受旧 artifact；当前选择用明确命令顺序和 recipe 注释管理，
  不为防呆引入 typed publication/freshness framework。Wrapper 必须执行完整顺序，验证结论也
  不能把路径存在当成同一 invocation 的证明。
- Platform kernel output 与 presentation 的边界若不清，仍可能把 U-Boot 格式错误移入 target
  或 preset；schema 必须保持 Platform 单一 owner。
- QEMU provider-generated embedded DTB会使host QEMU成为build input；R4接受这一点，但build必须每次从
  selected snapshot重新物化并记录完整命令，不能复用stale output或增加另一份committed baseline。
- QEMU bind若实际改变provider DT，省略bind的build-time dump会生成错误拓扑；R4A必须以两架构现有三个
  bind完成有/无attachment对照，并把DT-neutral作为后续Platform变更的停止条件。
- Embedded QEMU raw DTB可能携带build-time `/chosen/rng-seed`并使产物不可复现；R4A明确接受该限制，
  不保留仅为剥离该属性服务的canonicalization，也不把per-boot fresh seed列入当前保证。
- wrapper/docs/skill 若与 CLI/config cutover 分离，会留下两个有效工作流；同步必须是
  每个 gate 的验收项，而不是最终补文档。
- QEMU template若经过shell、按空格切分或不处理QEMU keyval分隔符，host path可能改变argv
  结构；第一版必须逐token传递并拒绝含逗号的path。
- QEMU bind 的完全人工映射不会提前发现内容正确性错误；wrapper与runtime验证必须保留足够
  的bind name/path诊断，不能把这项已接受边界误写成resolver compatibility保证。
- U-Boot post-link 属于 Platform build contract；若迁移中把它拆成独立 action或移入 target，
  会无必要地扩大 physical-board workflow。
- 本 RFC 内容较大；实施时仍需按最小可执行 gate 滚动解析，不能把 target 文档
  直接当成一次性 rewrite checklist。

## 收口

R5是当前accepted target。R0-R2的Stage 1-6、R3A与R4A均已关闭；R5不重开历史stage或Completed transaction，
并把从未解析或激活的R4B应用迁移从RFC acceptance boundary中移除。Stage 1已按用户授权完成，Checkpoint 1A-1D已依次独立关闭；独立的
`Stage 1 -> Stage 2 Implementation Resolution Gate`已把Stage 2解析为Ready，2A随后独立激活并关闭。该gate
确认ignored source-tree DTB使原Stage 2/3顺序无法直接成立，并在不改变R0 target的前提下把最小
normal-build DT输入前移为2A；QEMU refresh和剩余per-platform closure仍留在Stage 3。R0 已删除独立
package/output graph，并把 U-Boot固定为Platform-owned build post-link output；2B已以dormant foundation
关闭，2C已完成atomic production cutover，2D已完成integration/production validation并关闭Stage 2。
transaction记录实际checkpoint、resolution与R1 renegotiation证据；Stage 3与Checkpoint 3A已关闭，Stage 4与
单一Checkpoint 4A也已由独立授权完成和关闭；Stage 5/Checkpoint 5A与Stage 6/Checkpoint 6A均已独立关闭。
Stage 3关闭后的反馈确认physical baseline
provenance、允许差异与复核责任无法由
当前软件自动维护；R2删除没有action consumer的typed metadata，同时保留`provider = "firmware"`的
fail-closed行为和人类review义务。R3A随后关闭explicit-input cleanup。R4接受进一步owner纠偏：QEMU
provider不再拥有committed DTS mirror或maintenance CLI；R4A已原子完成schema、materialization、CLI与
repository surface cutover。R5确认该RFC没有剩余实施阶段并关闭。

## 修订记录

| 修订 | 日期 | 状态 | 语义变化 | Review / 事务 |
| --- | --- | --- | --- | --- |
| R5 | 2026-07-24 | Implemented / Closed | 将从未Ready或Active的R4B hardware-semantic Platform/adopter迁移移出本RFC；该变化不撤销R4A DT target、实现或验证，也不改变current contract。对应配置作为独立小迭代落地。 | [QEMU SMP Platform用途别名](../../devlog/changes/2026-07-24-qemu-smp-platform-aliases.md) |
| R4 | 2026-07-24 | Accepted for Implementation | QEMU Platform不再提交DTS或暴露DT refresh/check；firmware delivery使用runtime FDT，embedded delivery由normal build从selected provider自动生成build-local DTB；QEMU bind必须DT-neutral且build不提供占位符。Physical DTS source/baseline保持。R4A负责DT cutover并已关闭，R4B迁移SMP硬件Platform与业务target且保持Outline。 | [R4A QEMU provider DT cutover](../../devlog/transactions/2026-07-24-system-target-model-r4-qemu-dt.md) |
| R3 | 2026-07-24 | Implemented / Closed | 删除developer-local/repository-default selection和preset presentation defaults；system action只接受显式preset或完整tuple。Rootfs type、QEMU CPU和fmt scope显式；folder容量固定自动计算；BIOS保持optional且省略时不传`-bios`。 | [2026-07-24-system-target-model-r3-explicit-inputs](../../devlog/transactions/2026-07-24-system-target-model-r3-explicit-inputs.md) |
| R2 | 2026-07-23 | Accepted for Implementation | 接受Stage 3关闭后的配置反馈：physical firmware provenance、允许差异与复核责任保留为人类review事实，删除无action consumer的typed Platform metadata；DT authority/delivery、QEMU maintenance权限、runtime FDT与Contract Impact均不变。 | [2026-07-22-system-target-model](../../devlog/transactions/2026-07-22-system-target-model.md) |
| R1 | 2026-07-23 | Accepted for Implementation | 将DT delivery与authority解耦；LA64 embedded DTS改为QEMU provider-derived baseline；VisionFive以硬件经U-Boot导出的DTS为唯一baseline并删除未使用的官方副本，补齐physical firmware provenance、允许差异与runtime validation owner。Contract Impact保持不变。 | [2026-07-22-system-target-model](../../devlog/transactions/2026-07-22-system-target-model.md) |
| R0 | 2026-07-23 | Accepted for Implementation | 初始 accepted target；定义 system target、Platform、KernelConfig、BuildPreset、single resolved snapshot、Platform output 与 staged Boot Protocol Refine。 | [2026-07-22-system-target-model](../../devlog/transactions/2026-07-22-system-target-model.md) |
