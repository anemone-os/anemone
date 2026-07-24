# System Target Model Tracking Issues

**状态：** Closed（当前无 live design issue）
**最后更新：** 2026-07-24
**父 RFC：** [RFC-20260722-system-target-model](./index.md)
**迁移计划：** [迁移实施计划](./implementation.md)
**事务日志：** [R3 explicit-input cleanup](../../devlog/transactions/2026-07-24-system-target-model-r3-explicit-inputs.md)；
[R0-R2 history](../../devlog/transactions/2026-07-22-system-target-model.md)

本文只跟踪会改变 RFC target、owner、contract delta、实现顺序、review gate、停止边界或
验收判断的 confirmed design issues。普通实现选择、live-owner inventory、逐 platform
migration 和 validation evidence 不在这里持续保持 open；它们由 `implementation.md` 与未来
transaction 承接。若实现反馈要求改变 target invariant、ABI/runtime 语义、`Contract Impact`
或 acceptance boundary，应停止当前 gate 并在此重新建立 live issue。

## Apollyon

None.

## Keter

None.

## Euclid

None.

## Safe

None.

## Neutralized

| ID | 原问题 | Neutralize / 分流依据 |
| --- | --- | --- |
| `STM-R3-K1` | R2保留local selection -> tracked default fallback和future preset presentation defaults；rootfs type、QEMU CPU与fmt scope还存在省略驱动的策略选择。 | 用户将其接受为R3 target correction：system action只接受显式preset或完整tuple，删除local/default selection与preset presentation defaults；rootfs type、QEMU CPU和fmt scope显式。Folder容量统一自动计算；BIOS是有意保留的optional capability，省略只表示不传`-bios`。R3A负责原子清理与验证。 |
| `STM-R2-K1` | Stage 3把physical firmware provenance、允许差异与runtime validation owner做成三个只有唯一合法值的typed配置字段；它们没有action consumer，既不能证明capture来源，也不能执行板级复核。 | 用户将该审计结论接受为Stage 3关闭后的实现反馈。R2保留`provider = "firmware"`的authority分类与QEMU maintenance fail-close，把capture来源、只允许`/chosen/rng-seed`差异和板级/U-Boot变化后的复核责任恢复为baseline相邻说明与人类review证据，并删除对应parser/schema/test surface。DT delivery/authority、runtime FDT、current contract与Stage 4边界不变。 |
| `STM-R1-K1` | R0把DT delivery与authority绑定，误将QEMU-derived LA64 DTS分类为embedded normative；VisionFive physical baseline又只有generic firmware标签，缺少真实capture provenance、允许差异和runtime validation owner。 | Stage 3在latest-byte review后停止；用户确认LA64 machine-fact owner仍是QEMU，并确认`visionfive2-board.dts`来自supported硬件经U-Boot导出的runtime FDT且应删除未使用的官方DTS。R1将两维解耦，LA64改为embedded/provider-derived/QEMU；VisionFive closed metadata固定U-Boot hardware export、只允许`/chosen/rng-seed`差异并由Platform maintainer在板级/U-Boot更新时验证。该修订不改变runtime delivery或current contract。 |
| `STM-DRAFT-K1` | Resolved identity 与 provenance failure contract 尚未闭合 | 初版以 canonical semantic-input closure equality 收口，但该方向在 `STM-DRAFT-K8` review 中被确认粒度过粗、实现代价过高；当前有效修复由 K8 和 [STM-WORKFLOW-ORDER-001](./invariants.md#stm-workflow-order-001---固定路径依赖由明确命令顺序拥有) 取代，不再实施原 sidecar/digest/equality probe，也不承诺跨action freshness。 |
| `STM-DRAFT-K2` | Package output/backend 与 U-Boot owner handoff 尚未闭合 | 后续 review 证明问题前提过宽：当前只有 VisionFive Platform 要求的单一 legacy-image post-link，没有独立 package 抽象的真实压力。[STM-PLATFORM-OUTPUT-001](./invariants.md#stm-platform-output-001---platform-kernel-output-是-build-的一部分)现固定 U-Boot 为 Platform-owned normal-build output；独立 package CLI/backend/`[[outputs]]`已删除，Stage 4只核对字段推导与板级 Preserve。 |
| `STM-DRAFT-K3` | Platform manifest、DTS 与 runtime FDT authority matrix 未完成 | [STM-DT-001](./invariants.md#stm-dt-001---dts-authority-与-dtb-delivery-必须显式) 已固定唯一 authority 与 delivery target；完整 matrix 改为[迁移实施计划](./implementation.md) Stage 3 按 platform 滚动解析，未分类 platform 不得迁移。 |
| `STM-DRAFT-K4` | DT refresh/check 的 CLI、QEMU-only owner 与写入边界未闭合 | [STM-QEMU-DT-001](./invariants.md#stm-qemu-dt-001---dt-refresh-是-qemu-local-单管线维护-action) 已固定 `just qemu dt refresh --platform <qemu-platform> [--check]`：它与普通 execution 共用 QEMU namespace，但直接维护 QEMU Platform；default refresh 与 `--check` 共享单一 snapshot/canonicalization/compare 管线，只有 provider-derived conformance baseline 可原子写回，normative DTS fail-closed，physical platform 不建立通用 provider 抽象。验证与 per-platform 分类进入[迁移实施计划](./implementation.md) Stage 3。 |
| `STM-DRAFT-K5` | Boot Protocol baseline 与 EmbeddedApp 生命周期未闭合 | [BOOT-PROTOCOL-001](./invariants.md#boot-protocol-001---initial-program-source-统一收口到普通-vfs-exec) 已固定 ordinary VFS exec、materializer publication/cleanup 与 VFS reopen lifetime 的唯一责任；baseline 已在 public acceptance 前提取，mount/path/mode/materialization 由[迁移实施计划](./implementation.md) Stage 5 vertical slice 验证。 |
| `STM-DRAFT-K6` | QEMU完全人工bind无法证明SystemTarget role或先前action result | 用户明确接受当前阶段保持完全人工映射。[STM-QEMU-BIND-001](./invariants.md#stm-qemu-bind-001---qemu-bind-只参数化-tracked-argv-template)现只承诺declaration/map/path机械校验，并明确有效但内容选错的path可在QEMU/guest/wrapper验证中失败；不增加typed attachment/role/slot/result handoff，也不得把该边界误述为resolver已证明runtime artifact compatibility。 |
| `STM-DRAFT-K7` | Stage 1验证依赖Stage 2才选择的schema/reference与`inspect`接口 | [迁移实施计划](./implementation.md)已把最小canonical object schema、stable reference identity与resolver/Platform-output vertical slice前移到Stage 1 manifest；用户进一步确认第一版不需要inspect，现已删除该命令、JSON view与proof obligation。Stage 2不得建立第二resolver或改写已经进入snapshot的reference identity。 |
| `STM-DRAFT-K8` | Provenance使用完整resolution作为通用artifact identity，既错误禁止无关复用，又要求补齐昂贵的per-artifact/tool closure | 此后进一步收缩：跨action固定路径允许依赖明确命令顺序，不承诺typed publication/freshness。[STM-WORKFLOW-ORDER-001](./invariants.md#stm-workflow-order-001---固定路径依赖由明确命令顺序拥有)要求recipe/docs/wrapper写明顺序且验证运行完整流程，但不增加mtime、sidecar、invocation history或artifact graph；跨resolution cache、per-artifact closure与tool fingerprint均为非目标。 |
| `STM-DRAFT-K9` | Bind description、HostEnvironment resolver与inspect为第一版增加没有真实consumer的配置、CLI和验证面 | QEMU bind现只保留`name + argv template`；[STM-TOOL-001](./invariants.md#stm-tool-001---host-tool-按仓库固定程序名从-path-调用)要求xtask直接调用`qemu-system-*`/`dtc`/`mkimage`等固定程序名并依赖开发者`PATH`，不提供override/local binding/version/capability机制；inspect命令、JSON view和Stage 1 inspect slice已删除，实际action只打印必要的selection/resolution摘要。 |
| `STM-DRAFT-K10` | 实施草案只描述滚动冻结 write set，并把首个 Stage 1 manifest 推迟到 RFC acceptance 与 transaction creation 之后，无法满足接受时首个可执行阶段已经 Ready 的工作流边界 | [迁移实施计划](./implementation.md)现使用 `Outline / Ready / Active / Closed` 成熟度，并把初始 `Implementation Resolution Gate` 放在 public promotion 后、RFC acceptance 前；该 gate 同时解析 Stage 1 的完整交付、路线/probe、审计、可观测性、验证、停止/退出、cutover 与 manifest。Transaction 只在实现开始时记录证据、批准和链接，Stage 1 仍需单独授权才进入 Active。 |
| `STM-DRAFT-K11` | App build对已有binary/script的采纳边界与目标相反：草案要求source/copy路径“不使用no-op driver”，也没有区分build-command no-op、artifact export和runtime compatibility | [STM-APP-SOURCE-001](./invariants.md#stm-app-source-001---source-driver-只采纳已有-artifact)现固定closed Source driver：不启动build command，但复用公共path expansion、普通文件校验和export；不执行shell、转换或推断内容，不静默接受extra args。Stage 4验证Source/Cargo app build边界，Stage 5另行验证ELF/shebang经ordinary VFS exec/binfmt运行，避免把文件存在误述为runtime proof。 |
| `STM-DRAFT-E1` | 最小目录与 schema 命名尚未选择 | 最小canonical schema与reference identity由[迁移实施计划](./implementation.md)初始 `Implementation Resolution Gate` 选择，并在 Stage 1 Ready 定义中冻结；未参与identity的剩余目录组织、文件名、内部Rust类型和CLI形状保留为后续工程自由度，owner、host-path与single-resolver禁止退化项继续约束结果。 |
| `STM-DRAFT-E2` | Presentation defaults 需要预先给出白名单 | R2曾允许未来closed typed set；R3进一步删除该扩展点。[STM-PRESET-001](./invariants.md#stm-preset-001---preset-是选择器不是-overlay)现要求presentation input只能由本次action显式提供。 |
| `STM-DRAFT-E3` | Preset中的Cargo profile是否覆写app profile不明确 | 名称固定为`CargoProfile`，并在[STM-PRESET-001](./invariants.md#stm-preset-001---preset-是选择器不是-overlay)明确只选择kernel Cargo build profile、只作为kernel build input；app/rootfs task继续由自身manifest/driver拥有Cargo参数与profile。 |
| `STM-DRAFT-S1` | Final harness 具体接入尚未设计 | Final harness 已明确为 RFC 收口后的独立 adopter iteration；runner、scoring、image compatibility 与 local wrapper 不影响本 target 或首个 implementation stage。 |
| `STM-DRAFT-N1` | SystemTarget 与 BuildPreset owner 重叠 | Target拥有boot/deploy、root/entry selection与required capabilities；Platform拥有kernel output format；preset只选择target + KernelConfig + kernel-only `CargoProfile`，不携带presentation defaults。 |
| `STM-DRAFT-N2` | Final harness 被误当作 RFC 主目标 | RFC 主目标固定为通用 build/config/orchestration model；Final harness 只提供表达压力与后续 adopter。 |
| `STM-DRAFT-N3` | CLI、local selection 与 host tool binding 形成第二真相源 | R2先固定single resolver，R3再删除local/default source；[STM-CLI-001](./invariants.md#stm-cli-001---system-action-只有一条显式输入解析路径)只接受显式完整输入。[STM-TOOL-001](./invariants.md#stm-tool-001---host-tool-按仓库固定程序名从-path-调用)继续要求xtask调用固定程序名并依赖开发者`PATH`。 |
| `STM-DRAFT-N4` | QEMU runtime path 被过度泛化为跨 action binding | [STM-QEMU-BIND-001](./invariants.md#stm-qemu-bind-001---qemu-bind-只参数化-tracked-argv-template) 将该能力收缩为 platform-local tracked argv template + invocation value；不建立 provider-neutral role/slot/binding API。 |

上述 neutralize 只表示 RFC design issue 已有规范修复或受控实施归属，不表示对应代码、
validation、public acceptance 或 contract cutover 已完成。
