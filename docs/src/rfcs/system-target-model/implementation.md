# System Target Model 迁移实施计划

**状态：** R1（Stage 1-3 Closed；Stage 4 Outline / Not Resolved）
**最后更新：** 2026-07-23
**父 RFC：** [RFC-20260722-system-target-model](./index.md)
**目标与不变量：** [目标与不变量](./invariants.md)
**当前契约：** [`BOOT-PROTOCOL-001`](../../contracts/task/boot-protocol.md#boot-protocol-001--rootfs-metadata选择初始用户程序)；Refine target 尚未 cut over
**当前修订：** R1
**事务日志：** [2026-07-22-system-target-model](../../devlog/transactions/2026-07-22-system-target-model.md)

本文定义后续实施的 stage envelope、resolution/feedback gate、停止条件与回写路径。当前
RFC 已完成 public promotion、初始 `Implementation Resolution Gate` 与 R0 acceptance；transaction
已建立，Stage 1 已按用户授权完成全部 checkpoint 并独立关闭。独立的
`Stage 1 -> Stage 2 Implementation Resolution Gate`已完成；Stage 2的2A-2D已独立完成并关闭。
`Stage 2 -> Stage 3 Implementation Resolution Gate`已于2026-07-23完成；Stage 3已解析为一个有序
checkpoint，并在独立用户授权与activation preflight后执行、验证和关闭。Stage 4保持Outline / Not
Resolved；本轮没有运行其resolution gate。

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
- Stage 1 已由 `0B - Initial Implementation Resolution Gate` 完整解析为 `Ready`，在R0接受与
  transaction建立后按用户授权进入`Active`，并已完成全部checkpoint后关闭；Stage 2已通过独立
  resolution gate并完成2A-2D后关闭；Stage 3已通过独立resolution gate并完成3A后关闭；Stage 4至
  Stage 6仍为`Outline`。
- RFC acceptance、transaction creation 与 Stage 1 activation 是后续独立 gate。实现开始时建立的
  transaction 记录 accepted revision、preflight/批准证据、生效点和本文链接，不复制第二份计划或
  manifest；Stage 1从Ready进入Active所需的显式启动授权已经取得，其closure不激活Stage 2。
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
| Stage 1 | Closed | Resolver 与 Platform kernel-output vertical slice | Promotion preflight、public Draft review、0B resolution、R0 acceptance、transaction activation、1A-1D Closed | Closed；Stage 2 resolution gate已独立完成 |
| Stage 2 | Closed | QEMU normal-build DT前置、Selection、action scope与workflow surface cutover | Stage 1 Closed；Stage 1 -> Stage 2 resolution完成 | 2A-2D Closed |
| Stage 3 | Closed | QEMU DT refresh与剩余逐platform authority/delivery closure | Stage 2 Closed；2A baseline与Stage 2实际证据可审计 | 3A Closed；Stage 4 resolution gate未运行 |
| Stage 4 | Outline | Source app driver、app/rootfs workflow 与 physical-board closure | 相关 build foundation Closed | 前置实现证据足以解析 owner-local closure 后 |
| Stage 5 | Outline | EmbeddedApp vertical slice 与 Boot Protocol cutover | Resolver、app build 与 runtime input 稳定 | 前置阶段关闭后的独立 Implementation Resolution Gate |
| Stage 6 | Outline | Closure 与 adopter handoff | 前述实施阶段独立关闭 | 最后一个能力阶段关闭后 |

下表只登记当前可见的 checkpoint 候选轴，供各 Stage 的 Resolution Gate 判断体量和证明边界；它不是
当前分工、写入授权或已经冻结的 checkpoint 序列。

| 阶段 | 候选 checkpoint 轴 | Resolution Gate 必须特别判断 |
| --- | --- | --- |
| Stage 1 | canonical schema/reference；loader/resolver snapshot；kernel output 与 Platform post-link；定向验证和文档同步 | schema/reference 是否能先冻结且不被后续 consumer 重写；U-Boot Preserve proof 是否需要独立 review |
| Stage 2 | QEMU normal-build DT输入；selection source；build consumer；QEMU invocation/bind；CLI/help/schema/wrapper/docs cutover | DT前置能否在不实现refresh action的前提下切断normal-build QEMU依赖；每个用户可见interface能否原子cut over；旧selection/resolver是否会跨checkpoint残留为第二真相源 |
| Stage 3 | QEMU DT refresh、provider provenance与剩余platform authority/delivery审计 | 2A已分类baseline与实际diff是否足以解析refresh写入授权；若后一个platform必须依赖前一个实际证据，则改为独立滚动Stage，而不是未解析checkpoint |
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

**阶段成熟度：** Closed；R0 acceptance、transaction creation、Stage 1 activation及Checkpoint
1A-1D的独立review、验证与关闭均已完成。此后Stage 2已由独立resolution gate解析为Ready，但尚未激活。

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
- QEMU bind/CLI、DT authority/delivery、Source driver与EmbeddedApp在Stage 1关闭时分别受Stage 2/3/4/5保护。
  Stage 1不得修改它们的owner、public surface或acceptance boundary，也不得把QEMU-backed build
  误报为本vertical slice已关闭的action-scope证据。后续Stage 2 resolution因clean-checkout DT输入
  事实把最小normal-build DT单元前移到2A；该调整不反写Stage 1执行范围。

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

**状态：** Closed（2026-07-23）；执行、review与验证证据见
[transaction](../../devlog/transactions/2026-07-22-system-target-model.md#checkpoint-1b-execution-log)。
关闭没有自动激活1C。

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

**状态：** Closed（2026-07-23）；activation、执行、review与验证证据见
[transaction](../../devlog/transactions/2026-07-22-system-target-model.md#checkpoint-1c-execution-log)。

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

**状态：** Closed（2026-07-23）；activation、base-image preflight、执行、review与验证证据见
[transaction](../../devlog/transactions/2026-07-22-system-target-model.md#checkpoint-1d-closure---2026-07-23)。

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
  Stage 1关闭后的独立gate已按feedback规则把其中normal-build成立所需的最小DT输入前移到Stage 2的
  2A；该后续顺序调整不改变本停止条件当时保护Stage 1的事实。
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

**阶段成熟度：** Closed；`Stage 1 -> Stage 2 Implementation Resolution Gate`已于2026-07-23
独立完成，随后按用户授权依次激活并关闭2A-2D。Stage 2 closure未运行Stage 3 Resolution Gate。

### Stage 1 -> Stage 2 Resolution 结论（2026-07-23）

- Preflight读取了Stage 1最终diff与transaction review/validation、live Justfile/xtask help、resolver与
  config/task owner、全部tracked target/platform、两份pretest wrapper、cleanup入口、register/current
  limitations、R0 target/invariants与`BOOT-PROTOCOL-001`。Stage 1的single snapshot、SystemTarget root
  owner和Platform output边界保持成立，没有新shared runtime contract或live design issue。
- Live selection仍由root `kconfig [build]`、`conf switch`与wrapper共同改写；普通QEMU仍绕过resolver，
  独立读取Platform并接受`--platform/--image`。Platform `[qemu]`还保存host executable与硬编码runtime
  path，`build.name/abbrs`只服务legacy identity/selection。`clean`、`mrproper`、`xtask-clean`与
  `gendisk`的scope重叠。它们正是accepted current-to-target delta，不要求R1或新的Contract Impact。
- 两个本地`anemone-kernel/src/arch/*/generated.dtb`均被kernel `.gitignore`排除，clean checkout没有
  normal-build DTB输入。当前build又通过完整runtime QEMU argv执行`dumpdtb`；直接删除该调用会让
  LA64的`include_bytes!`失败，提交generated DTB则违反`STM-DT-001`。因此原Outline的Stage 2/3顺序
  不能原样执行。
- Gate按既有feedback规则把最小normal-build DT单元前移为2A：RV64的committed DTS分类为
  firmware-delivered provider-derived conformance baseline；LA64新增committed normative DTS并保持
  embedded delivery；两者只由normal build调用`dtc`生成`build/` output。2A不实现
  `qemu dt refresh`、baseline原子写回或完整provider provenance UI；这些仍由Stage 3解析。该调整只改变
  stage order/write set，不改变accepted DT authority/delivery target、runtime ABI或acceptance boundary。
- Stage 2体量与原子cutover要求不适合作为单一提交。Gate解析为2A DT prerequisite、2B dormant
  foundation、2C atomic production cutover、2D integration/closure。2B不得提前改变production CLI或
  tracked QEMU execution；2C必须在同一checkpoint删除全部legacy selection/QEMU入口并同步durable
  surfaces，不能留下兼容fallback。
- 第一版目录与文件冻结为`conf/build-presets/<slug>.toml`、tracked
  `conf/default-selection.toml`和ignored `conf/.selection.toml`。两种selection文件都只含
  `preset = "<BuildPresetRef>"`；`BuildPresetRef`沿用Stage 1 strict slug规则并唯一解析到preset目录。
  Preset只含target、workspace-relative KernelConfig与kernel-only `CargoProfile`，presentation defaults
  第一版为空。该命名是owner-local工程选择，不改变R0 target。
- Review没有形成新的Apollyon/Keter/Euclid。Register没有阻塞Stage 2的active build/boot issue；
  `BOOT-PROTOCOL-001`保持effective baseline，Stage 2 contract cutover为None。

### 受保护目标、schema 与 action closure

- Stage 1冻结的`SystemTargetRef`、`PlatformRef`、`KernelConfigRef`与owned
  `ResolvedSystemBuild`保持唯一实现。新增`BuildPresetRef`只增加selection source；explicit preset、
  complete low-level tuple、local/default preset ref三类来源互斥且每个action只resolve一次。
- `CargoProfile`从legacy `kconfig::Profile`移动到preset/selection owner；它只驱动kernel Cargo build。
  `KernelConfig` parser与tracked `.defconfig`删除整个`[build]`，`--disasm`保留为build action-local
  presentation option，不进入preset、snapshot或其它task。
- Tracked presets至少覆盖五个existing SystemTarget，并用同一`qemu-virt-rv64-pretest` target的dev与
  release preset证明一对多。Tracked preset统一引用`conf/.defconfig`，wrapper/CI显式选择preset；需要
  local `kconfig`的开发者使用complete low-level tuple，不把ignored path变成tracked preset隐式前提。
  Tracked default只选择一个preset，不复制其字段。
- BuildPreset TOML键固定为`target`、`kernel-config`和`profile`；selection TOML只允许`preset`。
  `conf/default-selection.toml`选择`qemu-virt-rv64-pretest-release`。两类parser都拒绝unknown field；
  第一版不建立presentation section。
- `conf list`保留为read-only canonical target/platform discovery并删除alias输出与`switch`；
  `selection show|set|clear`只读写`conf/.selection.toml`。Local file存在但invalid或引用missing时失败；
  只有文件缺席才回退`conf/default-selection.toml`。Explicit selection完全绕过这两个implicit source。
- `just build *args`与`just qemu *args`只转发xtask typed CLI。普通QEMU必须使用shared resolver，拒绝
  `--platform`、`--image`、raw args与host-tool override；physical Platform在resolve后明确报告unsupported。
- Platform `[qemu]`只保存fixed provider tokens与有序`[[qemu.bind]]`。每个bind包含strict name与token
  template：name沿用`[a-z0-9][a-z0-9-]*`，字段为`name`与`template = [<token>...]`；整个template必须
  至少包含一个`{{}}`，允许多次出现并全部替换为同一value。调用者提供`--bind name=path`。实现先拒绝unknown、duplicate、
  missing、empty、nonexistent/non-file与含逗号path，再按declaration order逐token替换并逐个传给
  `Command::arg()`，不得经shell或whitespace splitting。`--show-bindings`显示name/template后退出，
  不要求value、不启动QEMU。
- Tracked QEMU Platform使用机械名`kernel-image`、`disk-x0`与`disk-x1`。非pretest virtual Platform只
  声明实际需要的`kernel-image`；pretest按各自tracked device order声明三者。名字不编码SystemTarget
  role，wrapper仍负责把rootfs/test master映射到对应x0/x1。
- QEMU executable从selected architecture映射到固定`qemu-system-riscv64`或
  `qemu-system-loongarch64`；其它direct tool同样只按固定名从`PATH`调用。Platform删除
  `[qemu].qemu`、`build.name`与`abbrs`；filename slug继续是唯一Platform identity。
- Pretest wrapper只拥有master test disk只读输入的build-local copy、rootfs action顺序、显式preset、
  complete bind map、日志与host prerequisite。Runtime copy固定在`build/runtime/pretest-{rv64,la64}/`，
  不再解析/修改`kconfig`、调用`conf switch`、制造root-level sdcard或rootfs symlink、选择Platform或拼
  raw QEMU argv。
- Common cleanup只保留`just clean`/xtask clean，删除`mrproper`、`xtask-clean`与common `gendisk`。
  Clean删除repo-owned build/cargo/generated outputs，但不得删除用户`kconfig`或
  `conf/.selection.toml`；local selection只能由`selection clear`删除。

### Checkpoint 2A - QEMU normal-build DT prerequisite

**状态：** Closed；2B-2D Closed。

**交付：** 在Platform `[dtb]`中固定`source`、typed `delivery = "firmware" | "embedded"`、
`authority = "provider-derived" | "normative"`与provider-derived时必需的`provider = "qemu"`；output
path由build固定，不作为第二配置字段。保留现有RV64 DTS的文件身份与provider-derived baseline角色，
并在2A按current topology-only provider刷新已经确认drift的旧4-CPU/128-MiB内容；新增LA64 DTS。Normal build用
固定`dtc`把selected Platform的DTS编译到`build/generated/device-tree/platform.dtb`；该action-local
固定路径只代表当前snapshot且由后续build覆盖，不形成跨actionpublication或freshness contract。LA64 bootstrap
只嵌入该build output。删除normal-build `gen_qemu_cmd()/dumpdtb`与source-tree `generated.dtb`依赖；
2A完成后删除两个已确认由旧build生成且被ignore的`arch/*/generated.dtb`，后续clean也清理该legacy
output；不增加refresh CLI、QEMU bind或普通QEMU行为变化。

**Write subset：** `conf/platforms/{example.toml,schema.jsonc,qemu-virt-rv64.dts,
qemu-virt-la64.dts,qemu-virt-rv64.toml,qemu-virt-rv64-pretest.toml,qemu-virt-la64.toml,
qemu-virt-la64-pretest.toml}`；`scripts/xtask/src/config/platform.rs`、
`scripts/xtask/src/tasks/build/{mod.rs,device_tree.rs}`；
`anemone-kernel/src/arch/loongarch64/bootstrap.rs`；`conf/README.md`与build-system skill的
`references/{build-playbook.md,config-model.md}`。New LA64 DTS必须由topology-only provider dump取得并
记录该来源，不能从runtime disk配置反推。

**定向验证：** parser/schema test覆盖firmware/provider-derived与embedded/normative组合及非法组合；
`dtc`分别compile/decompile两个DTS，semantic compare当前topology-only QEMU dump；LA64 include路径只指向
本轮build output。使用PATH前置的必失败fake `qemu-system-*`并确保rootfs/test disk缺席，分别执行
RV64/LA64 QEMU-backed build；build仍成功且DTB位于`build/`，source tree不产生generated DTB。

**Review / Exit / Recovery：** reviewer确认唯一machine-fact owner、delivery与R0一致，2A没有实现或
伪造refresh capability。若现有RV64 DTS或新LA64 baseline无法与topology-only provider解释、kernel必须
继续读取source-tree generated DTB，或分类要求改变runtime FDT/target，停止并回DT owner/RFC review。
2A失败时在同一subset修复或完整恢复current QEMU dumpdtb path；不得留下一个arch新、一个arch旧的
normal-build行为。2A关闭只允许进入2B，不激活selection cutover。

### Checkpoint 2B - Dormant preset、selection 与 bind foundation

**状态：** Closed；2C-2D Closed。

**交付：** 增加strict `BuildPresetRef`、closed `BuildPreset`/`CargoProfile`、local/default selection parser、
shared selection resolver与QEMU bind declaration/map expansion单元测试；新增tracked preset/default/schema/
example和ignored local path。Production build仍从legacy bridge进入同一个Stage 1 resolver，普通QEMU及
tracked Platform runtime argv保持current behavior；不得暴露一半可用的新CLI。

**Write subset：** `.gitignore`、`conf/README.md`；新
`conf/build-presets/{example.toml,schema.jsonc,qemu-virt-rv64-release.toml,
qemu-virt-rv64-pretest-release.toml,qemu-virt-rv64-pretest-dev.toml,
qemu-virt-la64-release.toml,qemu-virt-la64-pretest-release.toml,visionfive2-rv64-release.toml}`、
`conf/{default-selection.toml,selection-schema.jsonc}`；
`scripts/xtask/src/{workspace.rs,config/{mod.rs,kconfig.rs,reference.rs,resolve.rs,platform.rs,
build_preset.rs,selection.rs},tasks/qemu.rs}`。`qemu.rs`中的新bind逻辑只由unit fixture调用；tracked
Platform与production `run()`不切换。

**定向验证：** 覆盖preset slug/path/schema、五target与同target dev/release matrix、missing
KernelConfig、explicit preset/tuple互斥、不完整tuple、explicit不读invalid local、local absent fallback、
local invalid/missing ref fail、snapshot mutation isolation；bind test覆盖declaration order、placeholder、
unknown/duplicate/missing/empty/non-file/comma path与逐token exact argv。Source audit确认legacy production
entry仍只有一个resolver，新增files尚未被build/QEMU行为消费。

**Review / Exit / Recovery：** reviewer确认preset没有overlay/presentation/QEMU value，local/default只含
preset ref，`CargoProfile`不流入app/rootfs；bind value不进入snapshot。若foundation要求改写Stage 1
reference identity或为兼容旧CLI建立第二resolver，停止Stage 2。2B失败时删除dormant files/types或在
subset内修复；current production behavior必须可恢复。2B关闭只允许进入原子2C。

### Checkpoint 2C - Atomic production CLI、QEMU 与 workflow cutover

**状态：** Closed；2D Closed。

**交付：** Build/QEMU切到shared selection args和single resolver；删除legacy `[build]` parser、
`resolve_legacy_build`、`conf switch`、Platform name/aliases、QEMU executable/hard-coded path与旧
`--platform/--image`。同一checkpoint切换tracked QEMU bind declarations、Justfile、selection CLI、
pretest wrappers与cleanup surface，并同步schema/example/docs/build skill。不得提交可运行旧入口的
compatibility alias或fallback。

**Write subset：** `Justfile`、`conf/.defconfig`、`conf/README.md`、
`conf/rootfs/visionfive2/README.md`、`conf/platforms/{example.toml,schema.jsonc,
qemu-virt-rv64.toml,qemu-virt-rv64-pretest.toml,qemu-virt-la64.toml,
qemu-virt-la64-pretest.toml,visionfive2-rv64.toml}`；`scripts/run-user-test-{rv64,la64}.sh`；
删除遗留的`scripts/qemu-virt-{rv64,la64}-dbg.just`；
`scripts/xtask/src/{main.rs,workspace.rs,config/{mod.rs,kconfig.rs,platform.rs,reference.rs,resolve.rs,
build_preset.rs,selection.rs},tasks/{mod.rs,conf.rs,clean.rs,qemu.rs,build/mod.rs}}`；删除
`scripts/xtask/src/tasks/mrproper.rs`；`.agents/skills/anemone-build-system/{SKILL.md,
references/build-playbook.md,references/config-model.md}`。2B新增tracked selection/preset files可在本
checkpoint修正，但不得增加新owner或schema类别。

**定向验证：** CLI parser与live help覆盖三类selection、`selection show/set/clear`、read-only
`conf list`、build `--disasm`及ordinary QEMU bind/show；fake fixed QEMU executable捕获并逐项比较RV64/
LA64 exact argv、bind declaration order、debug tokens与kernel/rootfs/test disk path。Legacy
`conf switch`、build `-k`、QEMU `--platform/--image`、`mrproper`、`xtask-clean`与`gendisk`必须稳定拒绝或
从help消失。两份wrapper通过`bash -n`与source audit，证明只使用explicit preset与complete bind map。

**Review / Exit / Recovery：** reviewer核对CLI/config/schema/wrapper/docs/skill latest bytes，确认没有旧
selection、host path、raw QEMU args或第二resolver残留。Cutover后失败只能在2C/full Stage manifest内修复，
或整体恢复到2B dormant/current production状态；不得只恢复某个wrapper、arch或legacy alias。任何
presentation改变bytes/task args、wrapper仍需semantic mutation或ordinary build/QEMU解析不同snapshot时
停止Stage 2。

### Checkpoint 2D - Integration、production validation 与 closure

**状态：** Closed；2C Closed；Stage 2 Closed。

**交付：** 对2A-2C latest bytes完成独立final review、完整validation floor与lifecycle/write-back；只在
frozen manifest内修复finding。2D不实现Stage 3 refresh action、app/rootfs新driver、EmbeddedApp或final
harness adopter。

**Write subset：** Stage 2 full manifest中的必要修复；
`docs/src/rfcs/system-target-model/{implementation.md,index.md}`、
`docs/src/devlog/transactions/2026-07-22-system-target-model.md`、
`docs/src/devlog/transactions/index.md`、`docs/src/rfcs.md`与当期biweekly devlog。只有stop/feedback命中时
才按批准结论扩大到`invariants.md`、tracking/register/current limitations/current contract；不得先改。

**Validation floor：**

1. `just xtask-test`通过selection/preset/snapshot/DT/bind/argv/cleanup定向测试；tracked JSON schema可解析，
   全部tracked target/platform/preset/default形成完整load matrix。
2. 在rootfs/test disk缺席且fake QEMU executable必失败的环境，分别使用explicit RV64/LA64 preset运行
   `just build --preset qemu-virt-rv64-pretest-release`与
   `just build --preset qemu-virt-la64-pretest-release`并成功，证明normal build不启动QEMU、不读取runtime
   bind；检查ELF、Platform output、`build/generated/device-tree/platform.dtb`与source-tree零generated
   artifact。另用`just defconfig`验证只重置local KernelConfig且内容不含legacy `[build]`；若测试前已有
   用户`kconfig`，必须先保存并在测试后逐字节恢复，不把validation-only状态提交。
3. 以fake fixed QEMU executable运行两个architecture的ordinary QEMU，验证exact argv与bind诊断；
   至少执行`just qemu --preset qemu-virt-rv64-pretest-release --show-bindings`和对应LA64命令，确认没有
   bind value时成功且不启动fake QEMU；再分别用`--bind kernel-image=... --bind disk-x0=...
   --bind disk-x1=...`捕获完整argv。Local selection matrix、explicit覆盖invalid local、low-level tuple及
   physical unsupported path均从live CLI执行。
4. `just --list`与build/qemu/conf/selection/clean live help、legacy CLI rejection、residual owner/path搜索、
   两份wrapper `bash -n`、write-set与ignored-local audit通过；ordinary clean后用户`kconfig`与
   `conf/.selection.toml`仍在，`selection clear`只删除后者。
5. 使用`./scripts/run-user-test-rv64.sh etc/final/images/sdcard-rv.img
   build/system-target-stage2-rv64.log`运行一次production RV64 pretest wrapper，确认master未修改、
   runtime副本只在`build/runtime/pretest-rv64/`、rootfs/build/QEMU按序执行并正常关机。LA64只要求同等
   syntax/fake-argv与build floor；真实LA64 QEMU、physical board、LTP全量与final harness均不作为Stage 2
   closure证据，除非另获授权并明确记录。
6. `git diff --check`、相对链接/状态残留审计与`mdbook build docs`通过；transaction准确记录实际命令、
   环境输入、Not Run与review结果，不把unit/build/fake-QEMU误报为guest/runtime证明。

**Review / Exit / Recovery：** final reviewer必须在latest bytes上确认Apollyon/Keter为零，Euclid要么修复
要么按批准路径分流；2A-2D全部独立关闭且上述floor满足后Stage 2才可标记Closed。缺master image、sudo、
host tool或runtime资源时2D保持Active并记录Not Run，不用旧日志替代。Stage 2 closure不自动运行
`Stage 2 -> Stage 3 Implementation Resolution Gate`。

### Stage 2 停止条件、contract 与 Resolved Write Set Manifest

- Normal build仍要求QEMU/rootfs/test disk/network backend，或LA64无法从build output嵌入committed DTS：
  停止在2A，不进入selection cutover。
- Wrapper/旧CLI必须继续改写semantic config，explicit与implicit selection需要字段merge，或build/QEMU
  无法共享同一个owned snapshot：停止并回owner review，不保留compatibility bridge。
- QEMU template必须经shell/空格切分、bind value必须进入preset/target/snapshot，或path机械校验开始
  声称证明artifact role/content：停止并回`STM-QEMU-BIND-001` review。
- Migration需要改变target/owner/ABI/visible semantics/acceptance boundary、新shared contract或
  `BOOT-PROTOCOL-001`：停止并进入RFC/Contract Impact review。普通stage order、文件布局与write-set调整
  先更新本文和transaction，不递增R0。

**Contract cutover：** None。Stage 2不得修改current contract；`BOOT-PROTOCOL-001` pending successor与
effective baseline保持不变。

**允许修改的现有production/config/workflow文件：** `.gitignore`、`.github/workflows/ci.yml`、
`.vscode/tasks.json`、`Justfile`、`conf/.defconfig`、`conf/README.md`、`conf/rootfs/visionfive2/README.md`；
`conf/platforms/{example.toml,schema.jsonc,qemu-virt-rv64.dts,
qemu-virt-rv64.toml,qemu-virt-rv64-pretest.toml,qemu-virt-la64.toml,qemu-virt-la64-pretest.toml,
visionfive2-rv64.toml}`；`anemone-kernel/src/arch/loongarch64/bootstrap.rs`；两份pretest wrapper；
`scripts/xtask/src/{main.rs,workspace.rs,config/{mod.rs,kconfig.rs,platform.rs,reference.rs,resolve.rs},
tasks/{mod.rs,conf.rs,clean.rs,mrproper.rs,qemu.rs,build/mod.rs}}`；build-system skill及两个references。

**计划新建：** `conf/platforms/qemu-virt-la64.dts`；
`conf/build-presets/{example.toml,schema.jsonc,qemu-virt-rv64-release.toml,
qemu-virt-rv64-pretest-release.toml,qemu-virt-rv64-pretest-dev.toml,
qemu-virt-la64-release.toml,qemu-virt-la64-pretest-release.toml,visionfive2-rv64-release.toml}`；
`conf/{default-selection.toml,selection-schema.jsonc}`；
`scripts/xtask/src/config/{build_preset.rs,selection.rs}`；
`scripts/xtask/src/tasks/build/device_tree.rs`。

**计划删除：** tracked `scripts/xtask/src/tasks/mrproper.rs`与不再使用、绕过shared selection/QEMU owner的
`scripts/qemu-virt-{rv64,la64}-dbg.just`；2A migration/后续clean删除已确认由旧build
产生的ignored `anemone-kernel/src/arch/{riscv64,loongarch64}/generated.dtb`。Root-level `kconfig`、local
selection与用户disk image不是deletion target；legacy fields/recipes只从其tracked owner删除。

**文档与execution write-back：** `docs/src/rfcs/system-target-model/{implementation.md,index.md}`、
transaction、transaction index、RFC navigation与当期biweekly devlog。`invariants.md`、tracking、register、
current limitations和current contract默认禁止修改；只有stop/feedback gate批准后扩展。

**Validation-only inputs（不得提交或手工修补）：** root `kconfig`、ignored `conf/.selection.toml`、
`etc/final/images/sdcard-rv.img`只读master、host `PATH`中的Rust/cross toolchain、`dtc`、固定名QEMU、
libguestfs/sudo、fake host-tool目录、temporary provider dump与全部`build/` output。Master image不得被
QEMU或rootfs工具原地打开为可写；wrapper只操作build-local copy。

**禁止触碰：** 其它kernel手写源码、SystemTarget schema/manifest、app/rootfs通用schema/task、
VisionFive U-Boot contract、Stage 3 QEMU DT refresh实现、Boot Protocol/current contract、其它RFC与final
harness。更合适的实现若要求越界，先报告文件、owner/contract影响、review/validation并更新manifest；
不得先改后追认。

**责任：** implementer按2A-2D维护single writer与transaction证据；build/config owner重点review DT输入、
selection与cleanup；QEMU owner重点reviewbind/token/argv；independent reviewer在每个cutover与final latest
bytes执行只读gate；integrator只在全部exit满足后同步Stage状态。Ready不向任何角色授予activation。

## Stage 3：QEMU DT refresh 与剩余逐 platform authority/delivery closure

**阶段成熟度：** Closed；`Stage 2 -> Stage 3 Implementation Resolution Gate`已于2026-07-23
独立完成。Gate把剩余工作解析为单一Checkpoint 3A；该checkpoint随后由独立用户授权执行、验证和关闭。

### Stage 2 -> Stage 3 Resolution 结论（2026-07-23）

- Preflight读取了Stage 2最终diff、2A/2D review与验证、live Platform parser/schema、全部6份tracked
  Platform、两份committed QEMU DTS、两份现存VisionFive DTS、normal-build DT pipeline、ordinary QEMU
  task/help、register/current limitations、R0 target/invariants与current transaction。Stage 2的single
  resolver、normal-build不启动QEMU、RV64 firmware baseline与LA64 embedded normative delivery均保持成立。
- 剩余实现只有同一个owner-local闭包：QEMU namespace新增显式DT maintenance branch，并完成全部tracked
  Platform的authority/delivery inventory。它不依赖某个平台checkpoint的运行反馈，因此无需拆成多个滚动
  Stage；为避免人为增加生命周期成本，Stage 3只冻结一个Checkpoint 3A。
- `qemu-virt-rv64`、`qemu-virt-rv64-pretest`与`example`共享同一provider-derived RV64 conformance
  baseline；`provider = "qemu"`授予check与mutating refresh。R1修订后，两份LA64 Platform同样以QEMU
  machine model为authority，committed DTS是embedded delivery的provider-derived baseline，不再把
  delivery误作normative authority。`visionfive2-rv64`是physical
  firmware-delivered Platform；现存`visionfive2-board.dts`作为firmware-derived conformance baseline纳入
  Platform DT contract，但不得获得伪QEMU refresh provider。另一份未被live Platform引用的
  `jh7110-starfive-visionfive-2-v1.3b.dts`按R1删除，不保留为并列machine-fact owner。
- Provider分类扩展为`qemu`与`firmware`，只服务Platform DT provenance和写入授权；它不增加generic provider
  API。QEMU mutating refresh capability仍只有`provider = "qemu"`；delivery不改变该写入授权，normative
  source仍只有check-only surface。Normal build继续对selected committed DTS执行
  `dtc -> build/generated/device-tree/platform.dtb`，firmware delivery不把该build output误述为runtime FDT。
- Review没有形成Apollyon、Keter或Euclid。该解析不改变kernel runtime FDT接受、root-mount ABI、target、
  public runtime API、shared contract、visible semantics或acceptance boundary；Contract cutover仍为None。

### R0 -> R1 Target Renegotiation Gate（2026-07-23）

Checkpoint 3A latest-byte review发现R0错误地从delivery推导authority：LA64 DTS虽然由normal build嵌入，
machine fact仍来自QEMU；同时VisionFive physical baseline缺少可表达的capture provenance、允许差异和runtime
validation owner。用户确认LA64保持QEMU authority，并确认`visionfive2-board.dts`来自当前supported硬件经
U-Boot导出的runtime FDT，应作为唯一baseline；未被live Platform引用且与硬件结果不同的官方
`jh7110-starfive-visionfive-2-v1.3b.dts`删除。

R1据此将delivery与authority解耦：RV64和LA64均为`provider-derived + provider=qemu`，各自保持firmware与
embedded delivery；VisionFive保持`firmware + provider-derived + provider=firmware`，closed metadata固定
`uboot-hardware-export` provenance、只允许volatile `/chosen/rng-seed`差异，并由Platform maintainer在板级/
U-Boot更新时验证runtime FDT。该修订改变RFC-local DT authority target与owner，故递增revision；它不改变
kernel runtime FDT接受、physical U-Boot handoff、root-mount ABI、public runtime API、shared current contract、
`BOOT-PROTOCOL-001` cutover或其它acceptance boundary。

本gate批准3A write set最小扩展为本RFC `invariants.md`、`tracking-issues.md`、两份LA64 Platform manifest和
删除上述未使用官方DTS；schema/parser/tests、current transaction及durable docs仍由原3A owner处理。验证增加
LA64 embedded/provider-derived check与mutating capability、VisionFive closed metadata正反例、被删DTS全仓零
consumer，以及既有DTS compile/build/QEMU-independent floor。Stage 3恢复Active；Stage 4保持Outline / Not
Resolved。

### Checkpoint 3A - QEMU DT maintenance 与完整 Platform DT closure

**状态：** Closed（2026-07-23）；执行、review与验证证据见
[transaction](../../devlog/transactions/2026-07-22-system-target-model.md#checkpoint-3a-closure---2026-07-23)。

**交付：** 在ordinary QEMU command下增加nested `dt refresh` branch，入口固定为
`just qemu dt refresh --platform <qemu-platform> [--check]`。该branch直接加载PlatformRef，不解析
SystemTarget、BuildPreset、KernelConfig、local selection或普通QEMU bind。QEMU provider使用只含machine、
CPU、SMP、memory与firmware选择的topology snapshot执行`dumpdtb`，不消费tracked runtime args、rootfs、test
disk、network backend或bind map。Baseline与provider output都经过同一个`dtc compile/decompile + volatile
/chosen/rng-seed removal + deterministic text canonicalization`管线后比较。

`--check`对QEMU provider-derived与QEMU-backed normative source复用同一管线：一致时成功，drift以专用
非零exit status和`DRIFT` diagnostic失败，config/tool/QEMU失败保持普通error status；所有临时文件位于
disposable目录并在成功或失败后清理。Default refresh显示semantic diff，只允许
`provider-derived + provider=qemu` baseline；存在drift时把包含当前provider/command provenance的DTS写入
同目录临时文件后原子rename。Normative DTS的mutating refresh、firmware provider与无DT/QEMU capability的
Platform均fail-closed，不建立任意`--output`或第二compare路径。

同一checkpoint把VisionFive Platform纳入`firmware/provider-derived` DT contract并同步schema/example、
build/config文档与build-system skill。6份tracked Platform的最终矩阵为：RV64 example/ordinary/pretest是
QEMU provider-derived firmware baseline；LA64 ordinary/pretest是QEMU provider-derived embedded baseline；
VisionFive是physical firmware-derived baseline且无QEMU refresh。共享source只表示同一canonical baseline，
不复制DTS。

**Write subset：** `conf/platforms/{example.toml,schema.jsonc,qemu-virt-rv64.toml,
qemu-virt-rv64-pretest.toml,qemu-virt-la64.toml,qemu-virt-la64-pretest.toml,visionfive2-rv64.toml}`；
删除`conf/platforms/jh7110-starfive-visionfive-2-v1.3b.dts`；`scripts/xtask/src/{main.rs,
config/platform.rs,tasks/qemu.rs}`；`conf/README.md`；`.agents/skills/anemone-build-system/{SKILL.md,
references/build-playbook.md,references/config-model.md}`；本RFC的`index.md`、`invariants.md`、
`implementation.md`、`tracking-issues.md`与current
transaction、transaction index、RFC navigation、当期biweekly devlog。`conf/platforms/*.dts`只在真实refresh
发现drift时允许由同一checkpoint更新；`kconfig`、local selection和`build/**`仅是validation-only状态。

**Validation floor：**

1. `just xtask-test`覆盖nested CLI、直接PlatformRef、capability/authority矩阵、topology-only exact argv、
   shared canonicalization、rng-seed removal、drift/error exit分类、normative check-only/physical fail-close与
   atomic update。
2. 对`qemu-virt-rv64`和`qemu-virt-rv64-pretest`分别执行真实
   `just qemu dt refresh --platform ... --check`；两者不得读取或要求runtime bind/path，baseline一致时退出0。
   用disposable fixture制造drift，证明check不写source且返回专用status；mutating refresh只更新fixture
   baseline并携带provenance；另以LA64 embedded/provider-derived Platform运行check并证明mutating capability，
   normative fixture保持check-only，任何QEMU maintenance模式都不能修改physical firmware source。
3. `dtc` compile/decompile RV64、LA64与VisionFive selected DTS；schema/parser覆盖全部6份Platform。至少一次
   RV64 normal build与一次LA64 normal build在PATH前置必失败fake QEMU且runtime disk/rootfs缺席时通过，
   证明embedded与firmware-delivered build都不启动QEMU；VisionFive用build-plan/DT materialization定向验证
   selected committed source，physical boot Not Run不计为Stage 3失败。
4. Live help、schema/example、config docs与build skill同步；`git diff --check`、relative-link/status/residual/
   write-set审计与`mdbook build docs`通过。Independent latest-byte review按owner、single pipeline、temporary
   cleanup、atomic write、provenance、exit classification与physical/normative fail-close复核。

**停止 / 恢复：**

- 需要kernel新增runtime FDT拒绝语义、改变root-mount ABI或形成新的跨RFC contract；
- 无法指出machine fact唯一owner，或必须让QEMU refresh反向改写normative/firmware source；
- check与mutating refresh无法共享canonicalization/compare truth，或action必须消费ordinary QEMU bind/runtime
  args才能物化topology；
- VisionFive baseline分类要求改变physical runtime FDT、U-Boot/firmware handoff或板级visible behavior；
- 当前Platform修复要求批量迁移未进入本manifest的其它owner。

命中上述信号时停止并进入RFC/`Contract Impact` review。未命中时，失败只在Checkpoint 3A frozen manifest
内修复；若真实refresh产生baseline drift，可以在同一checkpoint原子更新对应QEMU-derived DTS并记录证据。

**Contract / Stage Exit：** Contract cutover为None，`BOOT-PROTOCOL-001` effective baseline与pending successor
不变。Checkpoint 3A完成全部validation floor、independent review与transaction/RFC/durable-surface write-back后，
Stage 3才可Closed；关闭不运行或解析`Stage 3 -> Stage 4 Implementation Resolution Gate`。

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
