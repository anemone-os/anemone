# System Target Model 迁移实施计划

**状态：** R0（Stage 1 Active；Checkpoint 1A Closed）
**最后更新：** 2026-07-23
**父 RFC：** [RFC-20260722-system-target-model](./index.md)
**目标与不变量：** [目标与不变量](./invariants.md)
**当前契约：** [`BOOT-PROTOCOL-001`](../../contracts/task/boot-protocol.md#boot-protocol-001--rootfs-metadata选择初始用户程序)；Refine target 尚未 cut over
**当前修订：** R0
**事务日志：** [2026-07-22-system-target-model](../../devlog/transactions/2026-07-22-system-target-model.md)

本文定义后续实施的 stage envelope、resolution/feedback gate、停止条件与回写路径。当前
RFC 已完成 public promotion、初始 `Implementation Resolution Gate` 与 R0 acceptance；transaction
已建立，Stage 1 已按用户授权进入 Active，Checkpoint 1A 已独立关闭。本文冻结的 Stage 1
`Resolved Write Set Manifest` 仍不授权自动进入后续 checkpoint。

## 迁移原则

- Target invariant、唯一 owner、ABI/runtime 可见语义、`Contract Impact` 和 acceptance
  boundary 不由 implementation feedback 静默改变。实现若要求改变这些内容，当前 gate
  必须停止并回到 RFC review。
- 允许 implementation 决定目录和文件命名、内部 Rust 类型、Platform post-link函数组织，
  以及 `EmbeddedApp` 的具体 mount/path/mode/materialization 机制，
  只要它们满足已固定的行为边界。最小 canonical object schema 与 reference identity 必须在
  Stage 1 manifest 中冻结；一旦进入 resolver snapshot，就不能留到 Stage 2 重新选择。
- 第一版 presentation defaults 可以为空。只有出现真实重复 consumer 时才增加 closed、
  typed 字段；不为潜在需求建立开放字典、任意 args、overlay 或通用 workflow DSL。
- U-Boot surface 由 Platform 拥有，作为正常 `build` 的 post-link output Preserve current
  behavior。字段收缩必须证明可由 Platform 内其它真相源推导，且不改变 physical deployment
  contract；不得迁入 SystemTarget 或增加 package action。
- DT authority 按 platform 滚动分类。一个 platform 未完成 baseline 不阻止另一个 platform
  的 owner-local gate，但未分类 platform 不得迁移或声称完成。
- Probe 代码不得因“已经能跑”自然沉淀为长期抽象。只有证据支持、target 未改变且正式
  stage 接受其形状后才能保留；否则删除或改写。
- Final harness 仍是后续 adopter，不进入本 RFC 的通用 schema、首个 executable stage 或
  acceptance floor。
- 普通 QEMU bind 保持完全人工映射。Stage 1/2 只验证 declaration、map 与 host path 的机械
  正确性，不增加 product role/slot/prior-action-result handoff；语义选错但 path 有效的情况由
  QEMU/guest/wrapper 验证暴露，并保留 bind name/path 诊断。
- App `Source` driver 是 command no-op，不是 app action no-op。它不得启动shell或dummy command，
  但必须复用公共artifact path expansion、普通文件校验与export；binary/script的runtime兼容性
  继续由对应consumer和ordinary exec/binfmt验证。

## 反馈分流

| 反馈影响 | 处理位置 | 当前 gate 行为 |
| --- | --- | --- |
| 目录、内部类型、Platform post-link或 owner-local 实现路线 | transaction devlog；必要时更新本文 | 可继续，不改变 RFC 修订 |
| stage Outline/Ready 解析、顺序、write set、validation floor、review gate 或停止条件 | 本文 + transaction devlog | 先更新计划再继续 |
| target invariant、owner、ABI/runtime 语义、contract delta 或 acceptance boundary | `index.md` / `invariants.md` / `tracking-issues.md` | 停止并回 RFC review |
| effective contract | current contract + transaction cutover evidence | 仅在批准的 cutover gate 更新 |
| 已接受缺口或本应正确但失败 | current limitations / open issues | 不用兼容桥掩盖 |

## 阶段成熟度与滚动解析

- `Outline` 只固定概括目的、前置依赖、受保护边界和解析触发点；预计目录、模块、validation
  类别或当前已有的实现设想都只是后续 preflight 输入，不是冻结设计或写入授权。
- `Ready` 表示交付、实现路线或 probe、审计、可观测性、验证、停止/退出条件、contract cutover
  与 `Resolved Write Set Manifest` 已完整解析，但尚未自动获得执行授权。
- `Active` 表示已经通过 RFC / transaction / 用户或编排协议要求的启动授权；`Closed` 表示已经
  按本阶段自己的 review、验证和退出条件独立关闭。
- Stage 1 已由 `0B - Initial Implementation Resolution Gate` 完整解析为 `Ready`，并在R0接受与
  transaction建立后按用户授权进入`Active`；Stage 2至Stage 6仍为`Outline`，没有写入授权。
- RFC acceptance、transaction creation 与 Stage 1 activation 是后续独立 gate。实现开始时建立的
  transaction 记录 accepted revision、preflight/批准证据、生效点和本文链接，不复制第二份计划或
  manifest；Stage 1 仍需显式启动授权才从 Ready 进入 Active。
- 后续 Stage N 先独立关闭，再运行 `N -> N+1 Implementation Resolution Gate`。Preflight 必须读取
  live source、Stage N 实际 diff、review findings、模块边界压力、validation evidence、仍有效的
  RFC target/current contracts 与文档回写面，并把下一阶段的完整定义和 manifest 一起冻结。
- 本文是 Ready 阶段与 resolved manifest 的唯一权威。只有 Ready / Active 阶段冻结后的越界才属于
  write-set expansion；future Outline 的自然收窄、扩大、拆分、合并或重排属于滚动解析。

### 阶段内 checkpoint 解析

- Stage 是 target、owner boundary、contract cutover 与最终证明义务的闭合单元；checkpoint 只是同一
  Stage 内的执行、review、验证和可恢复停止边界。单个 checkpoint 通过不得被写成 Stage Closed、
  contract 已 cut over 或 target 已完成。
- 当前 Outline 只保留候选 checkpoint 轴，不提前冻结数量、名称、逐文件 write subset 或精确命令。
  对应 `Implementation Resolution Gate` 必须根据 live owner、实际模块边界、不可逆 cutover、验证时长、
  review/handoff 边界与失败隔离需求，决定该 Stage 能否作为单一执行单元；体量过大时，必须先解析为
  有序 checkpoint，再允许 Stage 达到 Ready。
- Stage 达到 Ready 前，本文必须同时冻结 checkpoint 顺序与依赖、每个 checkpoint 的交付、所属 write
  subset、定向验证、review/停止/恢复条件，以及覆盖全部 checkpoint 的 Stage 级
  `Resolved Write Set Manifest`。Transaction 只追加实际开始、关闭、失败与验证证据，不复制第二份
  checkpoint 计划或 manifest authority。
- Stage activation 不自动越过阶段内 checkpoint gate。前一个 checkpoint 满足自身退出条件并完成
  所需 review 后，才能按既有授权协议进入下一个 checkpoint；中途反馈若改变 checkpoint 顺序、
  validation floor 或 Stage manifest，必须先按“反馈分流”更新本文并取得所需 expansion/启动授权。
- 如果后续工作必须读取前一个 checkpoint 的实际 diff 或运行证据，才能解析自己的 owner、交付、
  write set 或停止条件，则它不能作为一个尚未解析的 checkpoint 藏在 Ready Stage 内。Resolution Gate
  必须把它提升为后续 Stage 或独立 probe，继续使用 Stage 间滚动解析；若还会改变 target、owner、
  contract 或 acceptance boundary，则先回 RFC review。

## 阶段路线图

| 阶段 | 当前成熟度 | 概括目的 | 前置依赖 | 解析触发点 |
| --- | --- | --- | --- | --- |
| Stage 1 | Active（Checkpoint 1A Closed） | Resolver 与 Platform kernel-output vertical slice | Promotion preflight、public Draft review、0B resolution、R0 acceptance、transaction activation | 1B仍为Not Started；按用户原始授权单独记录activation后执行 |
| Stage 2 | Outline | Selection、action scope 与 workflow surface cutover | Stage 1 Closed | `Stage 1 -> Stage 2 Implementation Resolution Gate` |
| Stage 3 | Outline | 逐 platform DT authority/delivery 迁移 | Stage 2 Closed；对应 platform baseline 可审计 | Stage 2 关闭后的 Stage 3 Resolution Gate；无法整阶段解析时在 gate 中拆成独立滚动 Stage |
| Stage 4 | Outline | Source app driver、app/rootfs workflow 与 physical-board closure | 相关 build foundation Closed | 前置实现证据足以解析 owner-local closure 后 |
| Stage 5 | Outline | EmbeddedApp vertical slice 与 Boot Protocol cutover | Resolver、app build 与 runtime input 稳定 | 前置阶段关闭后的独立 Implementation Resolution Gate |
| Stage 6 | Outline | Closure 与 adopter handoff | 前述实施阶段独立关闭 | 最后一个能力阶段关闭后 |

下表只登记当前可见的 checkpoint 候选轴，供各 Stage 的 Resolution Gate 判断体量和证明边界；它不是
当前分工、写入授权或已经冻结的 checkpoint 序列。

| 阶段 | 候选 checkpoint 轴 | Resolution Gate 必须特别判断 |
| --- | --- | --- |
| Stage 1 | canonical schema/reference；loader/resolver snapshot；kernel output 与 Platform post-link；定向验证和文档同步 | schema/reference 是否能先冻结且不被后续 consumer 重写；U-Boot Preserve proof 是否需要独立 review |
| Stage 2 | selection source；build consumer；QEMU invocation/bind；CLI/help/schema/wrapper/docs cutover | 每个用户可见 interface 能否原子 cut over；旧 selection/resolver 是否会跨 checkpoint 残留为第二真相源 |
| Stage 3 | 每个 platform 的 inventory、authority/delivery 迁移和定向验证 | 所有 platform 能否在 Stage Ready 前完整解析；若后一个 platform 必须依赖前一个实际证据，则改为独立滚动 Stage，而不是未解析 checkpoint |
| Stage 4 | Source driver/export；app/rootfs recipe；physical-board closure | 三者是否仍共享同一 owner/proof boundary；若 board closure 具有独立交付与验证 owner，则优先拆 Stage |
| Stage 5 | typed runtime input；materialization/cleanup；ordinary exec/binfmt handoff；Boot Protocol 验证与 cutover | cutover 前是否存在可安全停止的中间态；任何 probe 产物是否会形成第二 runtime truth |
| Stage 6 | residual surface audit；evidence/contract/register/docs closure；adopter handoff | closure 同步是否能保持原子，且 adopter handoff 不回填为本 RFC 的未关闭能力 |

## 首阶段前置 Gate：Live-owner inventory 与 Stage 1 解析

**Gate 状态：** `0A` Completed；`0B` Completed。该gate只把Stage 1解析为Ready；后续R0
acceptance、transaction creation与activation已独立完成。

本前置 Gate 分成两个顺序步骤；它们不是 Stage 1 的执行 checkpoint：

1. `0A - Promotion preflight` 已在 public promotion 时只读完成：提取 current baseline、核对
   Contract Impact 与 live owner，并把结论写回本 RFC 与 current contract。
2. `0B - Initial Implementation Resolution Gate` 在 public promotion 后、RFC acceptance 前执行，
   读取最新 live source、0A evidence、public Draft review 与 current contracts，判断并解析必要的
   阶段内 checkpoint，把 Stage 1 展开为完整交付、实现路线或 probe、审计、可观测性、验证、
   停止/退出条件和 contract cutover，并在
   本文冻结包含最小 canonical schema、reference identity 与 resolver vertical slice 的 Stage 1
   manifest；全部输出冻结后 Stage 1 才达到 Ready。

目标：

- 从 live deserialization、Justfile、xtask tasks、platform/rootfs/app manifests、wrapper 和
  kernel boot path 建立 current-to-target owner delta；不从目录名或现有字段位置推断 owner。
- 0A 已提取 `/.anemone/init -> absolute VFS path -> kernel_execve()` 的最小 effective
  baseline，供 public acceptance 时正确分类 `BOOT-PROTOCOL-001` Refine；本 public Draft
  不执行 target cutover。
- 在 0B 选择最小的 Stage 1 vertical slice，固定其 canonical object/reference 边界，完整解析
  Stage 1 并冻结 resolved manifest；Stage 2 不得重新定义已经进入 resolver snapshot 的
  reference identity。

审计输入：

- root `kconfig` / `conf/.defconfig` 与 `scripts/xtask/src/config/` live types；
- build/conf/qemu/app/rootfs actions 与用户可见 artifact exports；
- supported platform 的 DT delivery 入口和 VisionFive U-Boot post-link/rootfs fixed-path workflow；
- `exec_init_proc()`、rootfs init metadata producer、ordinary exec/binfmt/user-entry contract；
- wrappers、tracked schema/examples、build docs 与 `anemone-build-system` skill。

停止条件：

- live behavior 显示新的 shared runtime contract delta、第二个 mutable owner 或 target 未覆盖的
  public ABI；
- 首个 vertical slice 必须跨越尚未批准的 owner surface，或无法在最小 manifest 中验证；
- current baseline 无法支持 `BOOT-PROTOCOL-001` 的 Refine 分类。

输出与回写：

- 0A 的 baseline/contract 结论折回 public RFC；较长证据只在需要时进入具体命名的
  `backgrounds/` evidence packet。Stage 1 完整定义与 manifest 只写本文；transaction 建立后只
  追加 preflight/批准事实和本文链接；
- target/contract 问题写回 `index.md`、`invariants.md` 与 `tracking-issues.md`；
- 最小 schema/reference slice 的具体目录、文件名和内部类型由 Stage 1 preflight 选择并
  写入 manifest；未参与 canonical reference/snapshot 的剩余目录组织和 CLI 形状留给 Stage 2。

### 0A Promotion Preflight 结论（2026-07-22）

- root `kconfig` 的 `[build]` 由 `scripts/xtask/src/config/kconfig.rs` 解析，当前同时保存
  platform、kernel Cargo profile 与 disassembly；`conf switch` 只原地修改 platform 字段。
- `scripts/xtask/src/config/platform.rs` 与 `conf/platforms/*.toml` 当前共同承载 machine
  constants、root mount、QEMU command/argv、DTB source 和可选 U-Boot output；kernel build、
  QEMU 与 generated platform definitions 分别重读其中不同部分。
- kernel build 当前在 prebuild 中可通过完整 QEMU config 执行 `dumpdtb`，并在正常 build 内
  按 Platform `[uboot]` 执行 `objcopy -> mkimage`；这与本 RFC 的 current-to-target delta 一致。
- app manifest/parser 当前只有 Cargo driver；rootfs task拥有 app build/staging、文件系统
  composition 与 `/.anemone/init` metadata producer。
- RV64/LA64 pretest wrappers当前用`awk`读取root `kconfig`、必要时调用`conf switch`、重建rootfs、
  把调用者选择的只读master复制为固定worktree文件名，再以独立`--platform`/`--image`调用QEMU；
  tracked platform/rootfs examples与这些live parser/task字段一致。该事实确认wrapper仍是Stage 2
  必须同步移除的第二selection/presentation surface，不改变本RFC target。
- rootfs producer把 `[init].path` 原样写入 `/.anemone/init`；kernel `exec_init_proc()` 在 root
  mount 与 late init 之后读取该文本，准备初始 stdio/root/cwd，并调用 ordinary
  `kernel_execve()`。该 effective baseline 已提取为
  [`BOOT-PROTOCOL-001`](../../contracts/task/boot-protocol.md#boot-protocol-001--rootfs-metadata选择初始用户程序)。
- Public contract 中未发现第二项已被本 target 改变的 runtime shared rule；DT、root mount、
  build resolver 与 repository workflow 继续作为 RFC-local target/proof boundary。若 0B 或后续
  platform audit 发现额外 ABI/current-contract delta，必须按停止条件回到 RFC review。

### 0B Initial Implementation Resolution 结论（2026-07-22）

- Preflight 当时重新读取了 live `Justfile` / xtask help、config deserialization、build/conf/QEMU/app/
  rootfs tasks、全部 tracked Platform、root `kconfig` / `.defconfig`、RV64/LA64 pretest wrapper、
  VisionFive rootfs 固定路径，以及 kernel `mount_rootfs()` / `exec_init_proc()`。Register 没有与本
  stage 冲突的 active build/boot issue；RFC在0B执行时还没有transaction，符合acceptance前边界。
- Live delta 仍是 0A 已分类的同一组 owner migration：root `kconfig` 混合 selection 与
  KernelConfig，Platform 混入 root/QEMU host path，build/QEMU/conf/wrapper 分别重读 mutable
  selection，以及 QEMU DT prebuild 复用 runtime argv。没有发现新的 shared runtime contract、
  第二项 `Contract Impact`、target 未覆盖的 public ABI，`BOOT-PROTOCOL-001` baseline 也仍与 live
  producer/kernel handoff 一致；0B 停止条件未命中。
- Stage 1 选择 VisionFive 2 作为 Platform kernel-output vertical slice。它能同时验证
  SystemTarget -> Platform -> KernelConfig -> kernel-only `CargoProfile` reference closure、single
  resolver snapshot 与 Platform-owned U-Boot post-link，而不需要先迁移 QEMU bind、DT authority、
  app driver 或 EmbeddedApp。QEMU-backed Platform 只参与 resolver/config 回归，不作为 Stage 1
  “normal build 不依赖 runtime backend”的 closure evidence；其 DT build delta继续受 Stage 3保护，
  Stage 1 不得为通过验证而改写 QEMU DT recipe。
- Live tree 没有 VisionFive wrapper。为避免新建第二编排面，Stage 1 validation 把 Outline 中的
  “wrapper smoke”解析为仓库入口的显式 `build -> rootfs` 顺序、失败即停止的命令序列检查和最终镜像
  内容检查；永久顺序说明由相邻 README / recipe owner 保存。该调整只解析 validation route，
  不改变 `STM-WORKFLOW-ORDER-001` 或 acceptance boundary。
- Public Draft review 与本次 owner audit 未形成新的 Apollyon / Keter / Euclid；现有
  `tracking-issues.md` 可保持 Closed。以下 Stage 1 定义、checkpoint、review gate、validation floor、
  stop/exit condition 与 manifest 是 0B 的唯一 authoritative output。

0B 本身是docs-only resolution gate，实际write set只有本文件、RFC `index.md`与`docs/src/rfcs.md`
生命周期导航；没有创建transaction、修改current contract、读取backgrounds或授权production写入。
Review逐项核对了Ready完整性、checkpoint write subset/recovery、single-owner/reference边界、Stage 2/3
protected surface与manifest精确性；首轮发现的“checkpoint缺少独立write subset/recovery”和“条件性
new-file范围”已在本文修复，复核后无live finding。`git diff --check`、public相对链接审计、旧
Outline/0B状态残留搜索与`mdbook build docs`通过；mdBook只报告既有large search-index warning。
Kernel build、xtask test、QEMU、rootfs、physical board、LTP与runtime均Not Run，因为0B只关闭Stage 1
解析前置条件，不把Stage 1 validation floor误算为已执行。

## Stage 1：Resolver 与 Platform kernel-output vertical slice

**阶段成熟度：** Active；R0 acceptance、transaction creation 与 Stage 1 activation 已完成，当前只授权 Checkpoint 1A。

### 受保护目标与 scope envelope

- 建立能够自然表达 SystemTarget -> Platform、KernelConfig 与 kernel-only `CargoProfile` 组合的
  最小 canonical schema，并在本阶段冻结 reference identity；Stage 2 只能增加 selection source，
  不得重命名、并列实现或按 display/output name 重建这些 reference。
- 一次 build 只允许 loader/resolver 解析一次 canonical inputs并形成一个拥有完整值的 immutable
  `ResolvedSystemBuild`。Build consumer只接收 snapshot与 action-local presentation，不保留
  `KConfig`/Platform路径后再重读。
- SystemTarget 成为 root mount 与 `RootfsEntry` selection owner；Platform 保持 machine、DT、QEMU与
  kernel-output owner；KernelConfig只包含feature/policy/capacity。Stage 1 不改变kernel生成常量的值、
  root-mount runtime ABI或`BOOT-PROTOCOL-001`。
- VisionFive 2 是本阶段 production vertical slice：normal build按
  `kernel ELF -> rust-objcopy -> mkimage`生成现有`build/anemoneImage-rv64`，不读取rootfs/runtime
  backend，也不增加package CLI/backend/`[[outputs]]`。
- App/rootfs继续是直接action；VisionFive固定路径只由README/recipe与实际验证保存
  `build -> rootfs`顺序，不增加publication、freshness、artifact graph或history检查。
- QEMU bind/CLI、DT authority/delivery、Source driver与EmbeddedApp分别受Stage 2/3/4/5保护。
  Stage 1不得修改它们的owner、public surface或acceptance boundary，也不得把QEMU-backed build
  误报为本vertical slice已关闭的action-scope证据。

### Canonical schema 与 reference identity

- `SystemTargetRef` 是严格的仓库slug：`[a-z0-9][a-z0-9-]*`，唯一解析到
  `conf/system-targets/<slug>.toml`；不接受alias、display name、绝对路径、`..`或输出文件名。
  文件名就是identity，manifest不再保存第二个name字段。
- 第一版SystemTarget schema只包含`platform = "<PlatformRef>"`、`[root]`中的现有
  `fstype`/typed source，以及`[initial-program] type = "rootfs-entry"`。它不包含preset、QEMU
  bind、Cargo profile、rootfs recipe、artifact output或presentation字段。
- `PlatformRef`沿用`conf/platforms/<slug>.toml`的严格slug。现有`[build].name`在Stage 1只允许与
  文件slug相等并由assert/test校验；`abbrs`只服务尚未删除的legacy `conf`输入，不进入snapshot
  identity。Stage 2可以删除冗余name/alias surface，但不得改变slug identity。
- `KernelConfigRef`冻结为规范化的workspace-relative TOML路径；拒绝绝对路径、逃逸workspace的
  `..`和非普通文件。Root `kconfig`与tracked `conf/.defconfig`继续使用现有`[features]`/
  `[parameters]` schema；`[build]`由单独的legacy-selection parser消费，不属于KernelConfig值。
  Stage 2 public `--kernel-config`必须沿用同一规范路径identity，不能改成另一套slug registry。
- Stage 1把legacy `[build].platform`改名为`target`，并把它、kernel-only`profile`与`disasm`解析为
  `LegacyBuildSelection`/action presentation。该桥只让现有`just build`、`-k`与pretest wrapper在
  Stage 2原子CLI cutover前继续工作；它必须带诊断`selection source = legacy-kconfig`，不得被
  preset/local selection复用，并在Stage 2删除。
- `ResolvedSystemBuild`至少拥有target/platform/architecture、exact KernelConfig、kernel-only
  `CargoProfile`、`RootfsEntry`、root specification与本action所需Platform output/DT requirement。
  它不保存QEMU bind、disasm、host tool path、artifact digest/freshness或手写provenance。

### Checkpoint 1A - Schema、typed reference 与 dormant loader

**状态：** Closed（2026-07-23）；执行、review与验证证据见
[transaction](../../devlog/transactions/2026-07-22-system-target-model.md#checkpoint-1a-execution-log)。

**交付：** 新增SystemTarget parser/typed refs与五个对应当前supported Platform/root组合的tracked
manifest；新增纯loader/resolver单元测试。此checkpoint不切换production build，Platform中的legacy
root字段仍是唯一behavior source；新target文件保持dormant，避免中间态双写驱动行为。

**Write subset：** `Justfile`的private xtask test入口；新`conf/system-targets/**`；
`scripts/xtask/src/config/{mod.rs,reference.rs,system_target.rs,resolve.rs}`、`workspace.rs`，以及只为
parser/reference test需要的`config/{kconfig.rs,platform.rs}`。不修改build/conf/wrapper的production path。

**定向验证：** 通过repository-owned xtask test入口覆盖合法slug、path规范化、unknown/missing
target/platform/KernelConfig、filename/name不一致、unsupported initial-program tag和完整五target
load matrix；source audit确认production build尚未消费dormant target。

**Review / Exit：** reviewer确认schema没有preset/QEMU bind/output overlay，reference不依赖display
name；所有测试通过后才能进入1B。任何identity必须依赖Stage 2 CLI才能成立时停止整个Stage 1。
1A失败时删除dormant manifest/loader或在同一subset修复，production behavior保持当前状态后再恢复。

### Checkpoint 1B - Single resolver snapshot 与 build consumer cutover

**状态：** Not Started；不因1A关闭自动进入。

**交付：** legacy kconfig selection只调用一次resolver；build context改为消费owned
`ResolvedSystemBuild`。同一checkpoint把root生成输入切到SystemTarget并删除全部tracked Platform的
legacy root字段，防止cutover后双重truth；`conf switch`/pretest wrapper只更新legacy target ref，
不再把它描述为Platform owner。

**Write subset：** `conf/.defconfig`、`conf/README.md`、全部manifest中列明的Platform与SystemTarget；
`scripts/xtask/src/{workspace.rs,config/{mod.rs,kconfig.rs,platform.rs,reference.rs,system_target.rs,
resolve.rs},tasks/{build/mod.rs,conf.rs}}`；两个pretest wrapper。Root `kconfig`只作validation-only
迁移输入，不提交。

**定向验证：** resolver测试覆盖同一target的dev/release profile、mutation-after-resolve不会改变
snapshot、target/platform architecture与root-source错误；source audit列出所有`KConfig::from_str`、
`PlatformConfig::from_str`与`read_to_string(...platform...)`production caller，证明kernel build在
resolver后不重读。Action开始日志必须包含selection source、target/platform/KernelConfig refs、
profile和Platform output摘要，不打印整份配置或让diagnostic反向驱动行为。

**Review / Exit：** reviewer确认root只有SystemTarget owner、snapshot不是borrowed live config、legacy
bridge有Stage 2删除条件；repository xtask tests与无U-Boot build-plan test通过后才能进入1C。
Cutover后失败只能在同一subset中修复或整体恢复到1A dormant状态；不得保留target/platform双读来恢复。

### Checkpoint 1C - VisionFive Platform output production slice

**交付：** 在build owner内保留窄的U-Boot post-link实现，按ELF导出、`rust-objcopy`、`mkimage`顺序
生成Platform现有filename/header/load/entry/name。无`[uboot]`的Platform直接结束，不创建或声明本轮
U-Boot output；不增加backend、output registry、package命令或跨actionresult对象。

**Write subset：** `scripts/xtask/src/tasks/build/{mod.rs,kernel_output.rs}`与
`conf/platforms/visionfive2-rv64.toml`。本checkpoint不修改SystemTarget schema、QEMU task、rootfs task
或其它Platform behavior。

**定向验证：** 定向test检查有/无`[uboot]`的命令构造、顺序、参数与失败短路；在
`visionfive2-rv64` selection下通过仓库入口实际build并检查ELF、raw binary和legacy image均来自本轮。
`mkimage`缺失或失败必须指向程序名/action并使build非零。

**Owner review：** 复核U-Boot字段仍由Platform拥有，任何字段删除都有同owner派生证明，physical
header/load/entry/name/filename未改变。出现第二用户命令、target/package owner或generic transform
abstraction时停止Stage 1并删除该路线。失败恢复必须删除本checkpoint产生的partial raw/legacy
output并保留已关闭的resolver cutover；不能通过跳过Platform required output宣称build成功。

### Checkpoint 1D - Workflow、验证与文档同步

**交付：** VisionFive rootfs README/recipe明确同一selection的`build -> rootfs`顺序与host environment
变化后的clean责任；不创建VisionFive专用wrapper。同步受影响的Justfile/help、config example/schema、
pretest wrapper术语、build docs与`anemone-build-system` skill；transaction只记录实际证据并链接本文。

**Write subset：** `conf/rootfs/visionfive2/{rootfs.toml,README.md}`、Justfile/config example/schema、
两个pretest wrapper、`anemone-build-system` skill及其两份reference，以及本manifest列明的RFC/
transaction/navigation write-back。验证失败时保留已关闭code checkpoint但Stage 1不得Closed；修复只进入
失败owner的既有subset，越界则先扩展manifest。

**Validation floor：**

1. `just xtask-test`（Stage 1新增的private repository test recipe）通过全部config/reference/resolver/
   build-plan测试；不得用裸Cargo命令替代该入口。
2. `just xtask build -k kconfig`在本轮VisionFive legacy selection上成功，并检查
   `build/anemone.elf`、`build/anemoneImage-rv64.bin`、`build/anemoneImage-rv64`的freshness与顺序；
   构造失败的`mkimage` PATH fixture时build非零且后续rootfs命令不执行。
3. 使用开发者提供的只读/本地VisionFive base image，按文档完整执行build后再运行
   `just rootfs mkfs -c conf/rootfs/visionfive2/rootfs.toml --sudo`，通过`virt-ls`/等价只读检查确认
   `/boot/anemoneImage`存在且内容等于本轮Platform output。Base image、`mkimage`或sudo不可用时本
   checkpoint保持未关闭，不得用既有固定路径降级验证。
4. 再以一个无`[uboot]`的resolved fixture运行build-plan测试，证明不构造/调用`mkimage`；QEMU-backed
   production build、QEMU runtime、DT authority/refresh、kernel boot、LTP与final harness明确Not Run，
   不计入Stage 1 closure。
5. `git diff --check`、`mdbook build docs`、Stage 1 manifest边界审计、旧`build.platform`/Platform
   root owner/production direct-config-read残留搜索通过；live help、schema/example、wrapper/docs/skill
   与parser/task一致。

**Final review gate：** 独立只读review按owner、single snapshot、legacy bridge退出、U-Boot Preserve、
failure short-circuit、observability与write set逐项复核。Apollyon/Keter必须在Stage 1内neutralize或触发
停止；Euclid/Safe只有在不影响owner/acceptance且已写明归属时才可带入后续stage。

### Stage 1 停止条件、contract 与退出

- 实现需要重命名上述canonical reference、等待Stage 2才确定identity，或让Stage 2重写已进入
  snapshot的identity：停止并回0B/RFC review，不能保留双resolver。
- Live实现显示新的shared runtime contract、第二mutable owner、未登记public ABI，或root owner迁移
  会改变kernel生成值、root-mount/Boot Protocol可见语义：停止并扩大`Contract Impact`/review。
- 为让QEMU-backed build通过而必须修改DT authority、QEMU topology recipe/bind或runtime FDT边界：
  停止并保持该Platform current behavior，交给Stage 3 resolution，不把它塞入Stage 1 manifest。
- 开始比较跨resolution artifact identity，或引入typed publication、content-addressed cache、
  `[[outputs]]`、通用tool fingerprint/transform/backend：删除该路径并回到direct action/order模型。
- U-Boot必须迁入target/package、需要第二个用户命令，或physical Preserve无法证明：停止Stage 1并
  回owner review；不得用accepted limitation降低板级输出要求。
- 验证缺少base image、host tool、sudo或本轮fresh output时保持checkpoint未关闭并记录Not Run；不得
  以旧artifact、unit test或QEMU配置存在替代physical rootfs sequence floor。

**Contract cutover：** None。`BOOT-PROTOCOL-001` current baseline保持effective，Stage 1不得修改current
contract或Pending Successor状态。

**Stage Exit：** 1A -> 1B -> 1C -> 1D按序关闭，全部validation floor与final review满足，transaction
记录实际diff/证据/Not Run并完成RFC/implementation/durable-surface write-back后，Stage 1才可标记
Closed。Stage 1关闭不自动运行`Stage 1 -> Stage 2 Implementation Resolution Gate`。

### Resolved Write Set Manifest

**允许修改的现有production/config/workflow文件：**

- `Justfile`、`conf/.defconfig`、`conf/README.md`；
- `conf/platforms/{example.toml,schema.jsonc,qemu-virt-rv64.toml,qemu-virt-rv64-pretest.toml,
  qemu-virt-la64.toml,qemu-virt-la64-pretest.toml,visionfive2-rv64.toml}`；
- `scripts/xtask/src/{workspace.rs,config/mod.rs,config/kconfig.rs,config/platform.rs}`；
- `scripts/xtask/src/tasks/{conf.rs,build/mod.rs}`；
- `scripts/run-user-test-rv64.sh`、`scripts/run-user-test-la64.sh`；
- `conf/rootfs/visionfive2/{rootfs.toml,README.md}`；
- `.agents/skills/anemone-build-system/{SKILL.md,references/build-playbook.md,
  references/config-model.md}`。

**计划新建：**

- `conf/system-targets/{example.toml,schema.jsonc,qemu-virt-rv64.toml,
  qemu-virt-rv64-pretest.toml,qemu-virt-la64.toml,qemu-virt-la64-pretest.toml,
  visionfive2-rv64.toml}`；
- `scripts/xtask/src/config/{reference.rs,system_target.rs,resolve.rs}`；
- `scripts/xtask/src/tasks/build/kernel_output.rs`；它只承载现有U-Boot post-link，不得建立generic
  output/backend层。

**文档与execution write-back：**

- `docs/src/rfcs/system-target-model/{implementation.md,index.md,tracking-issues.md}`；
- activation时新建`docs/src/devlog/transactions/2026-07-22-system-target-model.md`，并同步
  `docs/src/devlog/transactions/index.md`、`docs/src/SUMMARY.md`与当期biweekly devlog；
- `docs/src/rfcs.md`只同步生命周期导航。只有命中stop/feedback分类时，才按批准结论修改
  `invariants.md`、register/current limitations或current contract；它们不在默认Stage 1写集。

**Validation-only inputs（不得提交或手工修补）：** root `kconfig`、ignored generated
`anemone-kernel/src/{kconfig_defs.rs,platform_defs.rs,arch/*/generated.dtb}`、VisionFive base image、
host `PATH`中的Rust toolchain/`rust-objcopy`/`mkimage`、libguestfs工具、sudo授权，以及`build/` outputs。

**禁止触碰：** `anemone-kernel/**`手写源码、`scripts/xtask/src/tasks/{qemu.rs,rootfs/**,app/**}`、
app/rootfs通用schema、DTS/DTB authority文件、QEMU bind/CLI、其它RFC/current contract、final harness与
competition资源。更合适的实现若要求越界，必须先报告新增文件、owner/contract影响、review与验证
计划并更新本manifest；不得先改后追认。

**责任：** implementer按checkpoint维护single writer与transaction证据；build/config owner复核schema、
resolver与U-Boot Preserve；independent reviewer只读执行final gate；integrator在所有证据满足后同步
Stage/RFC状态。Ready只冻结范围，不向任何角色授予activation。

## Stage 2：Selection、action scope 与 workflow surface cutover

**阶段成熟度：** Outline；Stage 1 Closed 后通过独立的
`Stage 1 -> Stage 2 Implementation Resolution Gate` 解析。

目标：

- build/qemu common-flow从Stage 1同一resolver snapshot取得各自输入；不得建立第二resolver或
  改写已冻结reference identity；
- explicit preset、完整 low-level tuple 与 interactive local preset reference 保持互斥的完整
  selection source；agent/CI/wrapper 只使用 explicit selection；
- host executable command/path从public config删除；xtask按action/architecture直接调用仓库固定
  程序名，由开发者`PATH`完成普通查找，不增加resolver、override或版本/capability机制；QEMU bind
  按target invariants保留在invocation；
- clean scope与CLI/help/docs同步明确：ordinary source/config变化由对应driver处理，改变未被
  driver/canonical config跟踪的隐式host build environment后由调用者执行相应clean；direct
  tool则由对应action每次实际调用；
- 每个用户可见 interface cutover 原子同步 live help、tracked schema/examples、wrapper、docs
  与 build skill，不保留长期双读或第二 resolver。

工程自由度：

- 未被Stage 1 manifest冻结为canonical reference/snapshot的目录组织、聚合/分文件布局、Clap struct与short
  flags由本stage preflight选择；Stage 1已经冻结的最小schema与
  reference identity只能扩展兼容的object集合，不能重命名或并列实现；
- 选择必须通过最小表达矩阵与 owner audit，不得为了兼容旧入口建立 overlay/fallback chain。

停止条件：

- build 仍要求 rootfs/runtime disk/network backend，或没有生成Platform声明的kernel output；
- wrapper 或旧 CLI 必须继续改写 semantic config 才能工作；
- presentation 字段开始改变 bytes、Platform output、guest contract 或 task调用参数；
- migration 需要改变 target/owner/acceptance，而不只是 stage order 或 write set。

Contract cutover：None。

## Stage 3：逐 platform DT authority/delivery 迁移

**阶段成熟度：** Outline；Stage 2 Closed 后通过独立的 Stage 3 Resolution Gate 解析。该 gate 先
inventory 全部 supported platform：若各 platform 的 owner、交付、write subset 与停止条件都能完整
解析，则把它们冻结为同一 Ready Stage 内的有序 checkpoint；若后一个 platform 必须依赖前一个
platform 的实际 diff 或验证证据才能解析，则在 gate 中把迁移提升为多个独立滚动 Stage，不只解析
首个平台便把 Stage 3 标为 Ready。

每个 platform 进入迁移前记录：platform kind/provider、manifest/DTS machine-fact owner、
committed DTS 角色、firmware/embedded delivery、normal-build 行为、QEMU DT refresh capability
及 baseline 写入授权、runtime FDT 接受边界与 validation owner。

Validation floor：

- 至少分别证明一个 embedded 与一个 firmware-delivered platform；
- normal build 不启动 QEMU dumpdtb，也不读取真实 rootfs/test disk；
- 双写字段具有明确派生方向或删除计划，“当前值相等”不作为 authority proof；
- QEMU-backed platform 通过 `just qemu dt refresh --platform <qemu-platform> [--check]`
  使用同一 snapshot/canonicalization/compare 管线；该 action 直接选择 platform，不读取
  SystemTarget/preset/KernelConfig、local selection 或普通 QEMU bind map；
- `--check` 只使用 disposable output，不写 source tree，并在 diagnostics/exit status 中区分
  baseline drift 与 config/tool/QEMU failure；
- mutating refresh 只原子更新 provider-derived conformance baseline 及其 provenance；
  normative DTS fail-closed，physical platform 不获得伪 refresh provider；
- 未进入本 gate 的 platform 保持 current behavior。

停止条件：

- 需要 kernel 新增 runtime FDT 拒绝语义、改变 root-mount ABI 或形成新的跨 RFC contract；
- 无法指出 machine fact 的唯一 owner，或必须让 QEMU refresh 反向改写 normative source；
- QEMU DT check 与 mutating refresh 无法共享 canonicalization/compare truth，或 action 必须
  消费普通 QEMU runtime bind 才能物化 topology；
- 当前 platform 的修复要求批量迁移其它尚未解析 platform。

这些信号触发 RFC/`Contract Impact` review；普通 per-platform inventory 和文件变化只更新本文
与 transaction。

## Stage 4：App/rootfs workflow 与 physical-board closure

**阶段成熟度：** Outline；Stage 3 对应迁移单元或相关 build foundation 关闭后，通过独立的
`Implementation Resolution Gate` 解析。

目标：

- app/rootfs继续拥有task-specific inputs/driver/output/failure contract，不拥有boot policy；
- 在现有closed `BuildDriver`中增加Source variant；driver阶段不创建command，app task仍对声明artifact
  执行统一path expansion、普通文件校验与export。不得以`true`等dummy process模拟no-op，也不得
  为Source建立第二套artifact收集路径；
- Source允许直接采纳已有binary、shebang script或其它普通文件，但不执行shell、不探测或修改内容/
  architecture/mode、不接受无处消费的额外driver args；缺失或非普通文件在export前失败；
- 跨action固定路径依赖只由recipe/docs/wrapper声明命令顺序，不增加package/backend/output graph；
- VisionFive U-Boot surface保留在Platform，`build`生成既有产物名并保持physical deployment behavior；
- 提交重命名、导出或完整介质装配由具体adopter/workflow拥有，不进入SystemTarget。

Owner preflight：

- 读取live `BuildDriver`、driver command dispatch、artifact copy/export与rootfs app staging owner，选择
  最窄的“可选command或direct source”内部形状；具体Rust类型/模块由本stage gate解析，不把Source
  扩展成generic command runner；
- 验证Cargo路径保持现状，Source binary与shebang script复用同一export结果；定向覆盖missing、
  directory/non-regular input、extra args和“没有子进程”证据。格式/architecture/runtime错误不在
  app build阶段伪装成已证明兼容；
- 核对 boot ABI、header/load/entry、format input、name、filename、firmware/bootloader handoff
  与 physical deployment，确认Platform schema和build post-link仍Preserve现有板级路径；
- 只在字段可由architecture/ELF/Platform其它唯一truth直接推导且不会改变行为时收缩；不能证明时
  保留当前字段，不为“整洁”制造新的owner；
- VisionFive rootfs recipe写明需要先运行同一selection的build；不增加mtime/history检查。

Contract cutover：None，除非 owner review 发现新的 shared contract；届时先回 RFC review。

## Stage 5：EmbeddedApp vertical slice 与 Boot Protocol cutover

**阶段成熟度：** Outline；依赖 resolver、app build 和 SystemTarget runtime input 稳定，并在前置
阶段关闭后通过独立的 `Implementation Resolution Gate` 解析。

受保护目标：

- `RootfsEntry` 与 `EmbeddedApp` 形成同一种有限 typed runtime input，最终都是稳定绝对 VFS
  path + argv/envp，并调用 ordinary `kernel_execve()`；
- `EmbeddedApp`只消费app task已导出的artifact；其来自Cargo或Source不改变materializer与kernel
  Boot Protocol。Source build成功只证明机械导出，不替代binary architecture、script shebang、
  interpreter或mode验证；
- materializer 在 publication 前独占创建与失败 cleanup，成功 handoff 后由普通 VFS 生命周期
  保证 exec/binfmt/interpreter reopen；
- kernel 不解析 target/preset/app manifest，不建立 anonymous-bytes loader、第二 binfmt、runtime
  registry 或 workload-specific branch；
- 现有 stdio/root/cwd、ordinary exec failure 与 PID 1 可见边界默认 Preserve。

Feedback hypothesis：

- 一个 ELF 与一个 shebang artifact 可以复用同一 VFS publication/handoff 模型；具体 mount、
  path、mode 与 materialization mechanism 可以保持 owner-local，不进入长期 contract。

Validation floor：

- source audit 证明唯一 materializer/VFS owner 与失败 cleanup；
- ELF 和 shebang 定向 boot smoke 均经过 ordinary exec/binfmt，interpreter reopen 成功；
- init artifact 缺失、publication 失败、interpreter 缺失和 reopen 失败具有明确可观察结果，
  且不残留可被下一次 boot 误认的 executable；
- repository build gate 与受影响 boot smoke 通过后，才允许 `BOOT-PROTOCOL-001` cutover。

停止条件：

- 需要改变 ordinary exec/binfmt/user-entry owner、引入第二 runtime truth，或削弱稳定 reopen/
  cleanup obligation；
- 具体机制要求改变 argv/envp、initial failure、PID 1 或其它未登记 runtime 语义；
- ELF 与 shebang 无法共享 target 所要求的 VFS path 模型。

任何停止信号先更新 RFC target、`Contract Impact` 与 tracking issue。Current baseline 在
cutover 前继续 effective；不能用 probe code 或 accepted limitation 提前覆盖。

## Stage 6：Closure 与 adopter handoff

**阶段成熟度：** Outline；前述 stages 独立关闭后通过最终的
`Implementation Resolution Gate` 解析。

- 审计旧 config/CLI/wrapper 是否仍形成第二 selection、resolver 或 host-path truth；
- 汇总每个 supported platform、app/rootfs workflow 和 Boot Protocol gate 的实际验证；
- 原子同步 public RFC 状态、transaction、affected contract IDs、register/current limitations、
  build docs/schema/examples/wrappers/skill；
- Final harness 只在本 RFC 通用接口收口后进入独立 adopter iteration，不回填通用 target。

本 stage 不以重复运行同一证据代替未完成的 owner/contract closure，也不因后续 adopter 尚未
实现而保持本 RFC transaction 开放。
