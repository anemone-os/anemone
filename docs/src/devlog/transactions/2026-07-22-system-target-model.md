# 2026-07-22 - System Target Model

**Status:** Active
**Owners:** doruche, Codex
**Area:** build system / configuration / platform / repository workflow
**Canonical Plan:** [RFC-20260722-system-target-model](../../rfcs/system-target-model/index.md), [目标与不变量](../../rfcs/system-target-model/invariants.md), [迁移实施计划](../../rfcs/system-target-model/implementation.md)
**Canonical Revision:** R0
**Current Phase:** Stage 1 Closed / Stage 2 Closed / Checkpoints 2A-2D Closed / Stage 3 Ready

## Scope

本事务执行R0的滚动实施。初始授权覆盖Stage 1前两个checkpoint：1A建立dormant SystemTarget
schema、typed reference与loader，1B完成single resolver snapshot和build consumer cutover；后续授权
已完成整个Stage 1。本轮授权覆盖Stage 2前两个checkpoint，但仍按2A、2B分别activation、review、验证、
回写和提交；前一个checkpoint关闭不自动启动后一个checkpoint。

后续用户明确授权完成Stage 2最后两个checkpoint。该授权仍按2C、2D分别activation、review、验证、
回写和提交；2C关闭不自动启动2D，2D关闭不自动运行Stage 3 resolution gate。

## Contract and register boundary

本Stage不执行current-contract cutover。`BOOT-PROTOCOL-001` R0 Refine保持pending successor，现有
rootfs metadata到ordinary `kernel_execve()`的effective baseline继续生效。2026-07-23 preflight读取
register、open issues与current limitations，未发现与Stage 1冲突的active build/boot issue。

## R0 acceptance and Stage 1 activation - 2026-07-23

用户明确授权完成Stage 1前两个checkpoint。Acceptance gate逐项核对RFC owner/target闭包、closed
tracking issues、已提取的Boot Protocol baseline、Stage 1 Ready definition与resolved manifest；未发现
新的Apollyon、Keter或Euclid design finding。R0被接受，transaction建立，Stage 1进入Active，但
activation point只开放Checkpoint 1A。

Stage 1 authoritative plan和manifest只位于RFC `implementation.md`，本事务不复制第二份write-set
authority。实现反馈若改变target invariant、owner、ABI、visible semantics、acceptance boundary或
需要越出frozen manifest，立即按停止合同上报；不得通过兼容桥或双重truth绕行。

**Validation:** acceptance/activation write-back运行`git diff --check`、相对链接/生命周期残留审计与
`mdbook build docs`。没有运行xtask tests、kernel build、QEMU、rootfs、physical board或LTP；这些
不是docs-only gate的完成证据。

## Checkpoint 1A execution log

**Status:** Closed

**Change:** 新增严格`SystemTargetRef`/`PlatformRef` slug与规范化workspace-relative
`KernelConfigRef`，新增closed SystemTarget schema、五个tracked dormant target manifest和可注入
workspace root的owned loader。Canonical KernelConfig值只包含features/parameters，不携带legacy
`[build]` selection；Platform filename identity与legacy`build.name`不一致时fail fast。

**Dormant boundary:** production build、conf、QEMU、main与两份pretest wrapper均不引用新loader或
SystemTarget。Platform legacy root仍由`gen_platform_defs()`消费，是1B原子cutover前唯一behavior
source；dormant target重复值不能驱动行为。没有创建`ResolvedSystemBuild` consumer、CLI、preset、
QEMU bind、output/publication graph或host-tool abstraction。

**Review:** 独立只读review逐项核对schema、identity、owned KernelConfig、path containment、五target
matrix、dormancy与write subset；最终实现无Apollyon、Keter或Euclid finding。Residual Safe是未建立
显式symlink-escape fixture，但loader已对workspace与candidate执行canonicalize并以
`starts_with(workspace_root)`直接拒绝canonical escape，本checkpoint要求的path-normalization覆盖满足。

**Validation:** `just xtask-test`在最终字节上运行20项测试，20 passed / 0 failed，覆盖严格slug、path
规范化、missing target/platform/KernelConfig、directory拒绝、Platform filename/name mismatch、
unsupported initial-program tag、owner-external字段拒绝、五target完整load/root matrix，以及legacy
selection变化不进入owned KernelConfig。`git diff --check`与全部新文件no-index whitespace检查通过；
source audit确认production零consumer。`just fmt xtask --check`因现有fmt task把standalone xtask误当根
workspace package而报`package xtask is not a member of the workspace`，未形成格式验证；本checkpoint
不越界修复fmt owner。Kernel build、QEMU、rootfs、physical board、LTP与runtime均Not Run，不能作为
1A证据。

**Result:** Checkpoint 1A Closed。没有命中identity、owner、ABI、visible semantics、shared contract或
write-set停止条件；`BOOT-PROTOCOL-001`保持effective baseline。Checkpoint 1B仍为Not Started，不由
本closure自动进入。

## Checkpoint 1B activation - 2026-07-23

用户原始授权明确要求完成Stage 1前两个checkpoint。1A已独立关闭并以`a22fb460`提交后，本事务
单独记录1B activation；该activation不来自1A自动推进。当前write subset、review/validation、恢复与
停止条件仍以canonical `implementation.md`为唯一权威。

Checkpoint 1B只完成single resolver snapshot、build consumer/root owner原子cutover与Stage 2必须删除
的legacy selection bridge。U-Boot post-link重构、workflow/durable-surface closure、QEMU/DT、Source
driver和Boot Protocol cutover均未获授权；1B关闭后不进入1C。

## Checkpoint 1B execution log

**Status:** Closed

**Change:** Legacy kconfig selection现在只调用一次resolver。Resolver在snapshot边界内解析selected
kconfig与必要的`.defconfig`默认值，把完整KernelConfig、SystemTarget、Platform、profile及canonical
refs固定到owned`ResolvedSystemBuild`；`gen_kconfig_defs()`不再在build consumer中重读默认配置。
Build只接收该snapshot与action-local`disasm`，并从SystemTarget root生成原有kernel常量。全部tracked
Platform已删除legacy root字段；`conf switch`与两份pretest wrapper只保留Stage 2必须删除的legacy
selection bridge。Wrapper显式区分SystemTarget与legacy QEMU Platform identity，不依赖同名巧合驱动
不同owner boundary。

**Review:** 首轮独立只读review发现三个Keter：defs生成在resolve后重读`.defconfig`、wrapper混用
target/platform identity、no-U-Boot测试没有连接实际post-link分支；另有一个RFC lifecycle Euclid残留。
修复后，resolver内物化全部参数默认值，temp-workspace mutation测试同时改写selected kconfig、`.defconfig`、
SystemTarget与Platform并证明snapshot/defs不变；wrapper拆分两种identity并写明Stage 2退出条件；实际
`build_uboot_image()`在构造任何command前先匹配被测试的`UbootPlan::Skip`。复核无Apollyon、Keter或
Euclid finding，20个modified file仍全部位于冻结1B/Stage 1 manifest。

**Validation:** 最终字节运行`just xtask-test`，26 passed / 0 failed，覆盖五target matrix、同一target
dev/release profile、invalid architecture/root source、完整默认值物化、canonical mutation后的owned
snapshot不变、root owner cutover及no-U-Boot skip分支。`just xtask conf list`、两份wrapper的`bash -n`、
`git diff --check`与source/residual audit通过；audit确认build没有`KConfig::from_str`、
`PlatformConfig::from_str`或Platform路径直读，`.defconfig`只在resolver内作为默认输入读取，tracked
Platform不再存在`[rootfs]`，production也没有`build.platform`残留。`mdbook build docs`通过，只报告
既有large search-index warning。`just fmt xtask --check`仍在执行格式检查前因现有root workspace不包含
standalone`xtask`而失败，未形成format validation；本checkpoint不越界修复fmt owner。Kernel build、
QEMU、rootfs、physical board、LTP与runtime均Not Run，不计入1B证据。

**Result:** Checkpoint 1B Closed。没有命中owner/API/shared-contract/ABI/visible-semantics/
target-invariant/write-set停止条件；Stage 1仍为Active，Checkpoint 1C保持Not Started且未获授权。
`BOOT-PROTOCOL-001`继续由effective baseline生效，本checkpoint没有contract cutover。

## Checkpoint 1C activation - 2026-07-23

用户本轮授权扩展为完成整个Stage 1。Checkpoint 1B已独立关闭并以`f2a0af4a`提交后，本事务
单独记录1C activation；该activation来自新的Stage 1目标，不来自1B closure自动推进。

Checkpoint 1C只在build owner内收窄现有U-Boot post-link并补齐其定向测试与真实VisionFive
build证据。Platform字段和physical output contract保持不变；SystemTarget schema、QEMU/DT、rootfs、
Source driver、package/output registry与Boot Protocol均不进入本checkpoint。

## Checkpoint 1C execution log

**Status:** Closed

**Change:** 现有U-Boot post-link从`build/mod.rs`收窄到build-owner私有`kernel_output.rs`；
normal build在导出`build/anemone.elf`后按固定`rust-objcopy -> mkimage`顺序生成raw与legacy image。
参数继续全部来自Platform `[uboot]`，VisionFive的header/load/entry/name/filename未改变；无`[uboot]`
直接结束。Post-link开始前清除旧raw/legacy output，任一步spawn或非零失败后再次清理partial output，
错误同时报告U-Boot action与程序名；主构建失败也不再继续`postbuild`。

**Review:** 独立只读review按Platform owner、窄模块边界、固定命令顺序、无U-Boot skip、失败清理、
诊断和write subset复核。首轮指出失败后仍运行no-op `postbuild`的Euclid，以及transaction Scope把初始
1A/1B授权误写为当前授权的lifecycle Euclid；两项均在本checkpoint修复。Final-byte复核无Apollyon、
Keter或Euclid finding；没有出现package/backend/output registry、target-owned U-Boot或shared-contract
delta。

**Validation:** 最终字节运行`just xtask-test`，30 passed / 0 failed；新增5项测试覆盖完整
objcopy/mkimage argv与顺序、无U-Boot skip、objcopy失败短路、mkimage失败、partial cleanup及缺失工具
诊断。Validation-only root `kconfig`临时从`qemu-virt-rv64-pretest`切到`visionfive2-rv64`并在验证后
恢复；`just xtask build -k kconfig`成功，生成的ELF、raw与legacy image时间戳均晚于本轮marker。
`mkimage -l`确认名称、RISC-V Linux kernel/uncompressed类型、`0x80200000` load/entry保持不变；
`dumpimage`提取payload与本轮raw binary逐字节相等。首次沙箱内build因cross-compiler收到`SIGSYS`
失败，获批后在非沙箱环境用同一仓库命令重跑成功，不将环境失败记录为代码通过或失败。
`git diff --check`和ignored新文件no-index whitespace检查无诊断；`mdbook build docs`通过，仅报告既有
large search-index warning。`just fmt xtask --check`仍在rustfmt前因既有root workspace不包含standalone
xtask而失败，未形成format validation。Rootfs sequence、physical board、QEMU/runtime、DT、kernel boot、
LTP与final harness均Not Run，不计入1C证据。

**Result:** Checkpoint 1C Closed。没有命中owner/API/shared-contract/ABI/visible-semantics/
target-invariant/write-set停止条件；`BOOT-PROTOCOL-001`继续由effective baseline生效且无contract
cutover。Checkpoint 1D保持Not Started，不由本closure自动进入。

## Checkpoint 1D activation - 2026-07-23

本轮用户授权要求完成Stage 1。Checkpoint 1C已独立关闭并以`61b4179c`提交后，本事务单独记录1D
activation；该activation不来自1C closure自动推进。1D只同步Stage 1已经改变的schema/example、
VisionFive固定路径workflow、build skill与lifecycle/navigation，并执行完整Stage validation floor；
不进入Stage 2 resolution或修改current contract。

Preflight确认host `rust-objcopy`、`mkimage`、`virt-ls`与`mdbook`可用，但developer-local
`conf/rootfs/visionfive2/rootfs-alpine.img`在alpha、omega、旧checkout与共享树中均不存在。按Stage 1
停止条件，1D可以继续完成其它交付和验证，但在提供真实base image并完成physical rootfs sequence前
必须保持Active，不能用competition image、旧rootfs或unit test替代。

## Checkpoint 1D progress - 2026-07-23

**Status:** Active；physical rootfs validation pending。

**Change:** 新增与live closed SystemTarget parser一致的`example.toml`/`schema.jsonc`；修正Platform
example的filename identity、移除已迁出的stack/heap字段并使RV64 DT示例自洽，Platform schema补齐
live required/optional字段与U-Boot post-link section。VisionFive recipe/README现在明确同一selection的
`build -> rootfs`顺序、失败短路及host environment变化后的build前clean责任。Build skill及两份
reference已同步SystemTarget、Platform kernel-output与fixed-path order owner；transaction index、RFC
navigation和biweekly entry已从旧1A状态更新为1D Active。Justfile/live help、两份pretest wrapper、
`conf/README.md`与SUMMARY经审计可Preserve，未制造无行为变化的编辑。

**Validation completed:** `just xtask-test` 30 passed / 0 failed；`jq empty`验证两份JSON schema，
`just xtask conf list`与validation-only `conf switch example -> qemu-virt-rv64-pretest`证明example
reference可由live parser/discovery消费且selection已恢复。`just --list`、build/rootfs live help、两份
wrapper `bash -n`、`git diff --check`、lifecycle/residual owner搜索通过；`mdbook build docs`通过，仅报告
既有large search-index warning。使用PATH前置的失败`mkimage`执行真实VisionFive build，build以23非零
退出，错误包含`build U-Boot legacy image`与`mkimage`，`&&`后的sentinel未创建，raw/legacy partial均被
清理；随后validation-only selection已恢复。该build在获批的非沙箱环境运行，避免cross-compiler
受沙箱`SIGSYS`影响。

**Pending floor:** developer-local `conf/rootfs/visionfive2/rootfs-alpine.img`仍不存在，因此尚未重新
运行成功VisionFive build及`just rootfs mkfs -c conf/rootfs/visionfive2/rootfs.toml --sudo`完整顺序，
也未用`virt-ls`/`virt-cat`证明`/boot/anemoneImage`与本轮Platform output相等。按停止条件1D/Stage 1
保持Active，不提交checkpoint closure。QEMU-backed production build、QEMU runtime、DT authority/
refresh、kernel boot、physical board boot、LTP与final harness均Not Run，不计入Stage 1证据。

## Checkpoint 1D closure - 2026-07-23

**Status:** Closed；Stage 1 Closed；Stage 2 resolution gate Not Entered。

**Base-image resolution:** 开发者确认本地决赛Debian raw ext4镜像可作为本次只验证image copy与
fixed-path注入的只读base。验证通过ignored local入口把既有tracked recipe解析到该master；rootfs task
先复制base，再只修改`build/rootfs/visionfive2/rootfs.img`。Master与生成镜像的device/inode不同，
master时间戳保持不变。该替代只满足本checkpoint packaging floor；未把Debian userspace记为Alpine
musl/native-tool环境、kernel runtime或physical-board证据。

**Physical rootfs sequence:** Validation-only selection切到`visionfive2-rv64`，紧接同一文档顺序运行
`just xtask build -k kconfig`与
`just rootfs mkfs -c conf/rootfs/visionfive2/rootfs.toml --sudo`，两步均成功；随后selection恢复为
`qemu-virt-rv64-pretest`。ELF、raw与legacy image均晚于本轮marker，大小分别为10124632、3474704、
3474768 bytes。`mkimage -l`确认`Anemone OS for RISC-V`、RISC-V Linux kernel/uncompressed、
`0x80200000` load/entry；`dumpimage` payload与raw逐字节相等。等价只读`debugfs`检查确认生成rootfs的
`/boot/anemoneImage`为3474768 bytes，抽取文件与本轮`build/anemoneImage-rv64`逐字节相等，SHA-256
均为`c98358afe1a33943f3cefd1888a79bdbfb16acf0fc6e9b5673fc173037dc2f53`；`/.anemone/init`为`/sbin/init`。

**Review and final validation:** Final review preaudit无Apollyon/Keter，唯一Euclid是RFC implementation
导言仍只写1A Closed；已修正为当时的1A-1C Closed / 1D Active，再同步本closure。Latest-byte独立
复核为Apollyon 0 / Keter 0 / Euclid 0；SystemTarget与Platform schema/example、single snapshot/
legacy bridge、U-Boot owner与失败短路、workflow/skill、write set及contract边界均无阻塞finding。
最终字节`just xtask-test`为30 passed / 0 failed；两份schema
通过`jq empty`，live `conf list`、`example -> qemu-virt-rv64-pretest` switch、build/rootfs help、两份
wrapper语法、residual owner/lifecycle/write-set/whitespace审计与`git diff --check`均通过，selection已
恢复。`mdbook build docs`通过，仅报告既有large search-index warning。QEMU-backed production build、
QEMU runtime、DT authority/refresh、kernel boot、physical board boot、LTP与final harness仍Not Run，
不计入Stage 1 closure。

**Result:** Checkpoint 1D与Stage 1 Closed。Stage 1没有命中target/owner/API/shared-contract/ABI/
visible-semantics/write-set停止条件，contract cutover仍为None；`BOOT-PROTOCOL-001` effective baseline
不变。按Stage Exit，本closure不运行或解析`Stage 1 -> Stage 2 Implementation Resolution Gate`，Stage 2
保持Outline。

## Stage 1 -> Stage 2 Implementation Resolution - 2026-07-23

**Status:** Completed；Stage 2 Ready / Not Activated。

本gate在Stage 1独立关闭后只读执行，重新读取Stage 1最终diff、transaction review/validation、live
Justfile与xtask help、config/resolver/build/QEMU/conf/clean task、全部tracked target/platform、两份pretest
wrapper、register/current limitations、R0 target/invariants与current contract。Stage 1的single snapshot、
SystemTarget root owner、Platform output和`BOOT-PROTOCOL-001`边界保持成立；没有新shared runtime contract
或live design issue。

Preflight确认Stage 2需要收口的live delta仍是legacy `kconfig [build]`、`conf switch`、独立QEMU
selection、Platform host executable/runtime path、wrapper semantic mutation与重叠cleanup surface。另发现
两个本地`anemone-kernel/src/arch/*/generated.dtb`均被kernel `.gitignore`排除；clean checkout不能在删除
normal-build QEMU `dumpdtb`后继续完成LA64 `include_bytes!`。提交generated DTB违反`STM-DT-001`，保留
QEMU又违反Stage 2 action-scope exit，因此原Stage 2/3顺序必须在保持target的范围内修正。

Gate把最小normal-build DT prerequisite解析为Checkpoint 2A：RV64 committed DTS是firmware-delivered、
provider-derived baseline；LA64补齐committed normative DTS并保持embedded delivery；normal build只用固定
`dtc`产生`build/` DTB。QEMU refresh、baseline mutating/check与剩余per-platform provenance/authority closure
仍留在Stage 3。随后依次冻结2B dormant preset/selection/bind foundation、2C atomic CLI/config/QEMU/
wrapper/cleanup cutover、2D integration/production validation与closure。该变更属于stage order/write-set
解析，不改变R0 target、owner、ABI、visible semantics、acceptance boundary或Contract Impact。

Stage 2的authoritative checkpoint定义、validation floor、stop/recovery、contract cutover与完整manifest只
位于[迁移实施计划](../../rfcs/system-target-model/implementation.md#stage-2selectionaction-scope-与-workflow-surface-cutover)。
本事务不复制第二份计划authority。Resolution review没有Apollyon、Keter或Euclid；Stage 2达到Ready，
但没有获得activation授权，2A也未开始。Contract cutover仍为None，`BOOT-PROTOCOL-001` effective baseline
及pending-successor状态不变。

**Resolution validation:** `git diff --check`、lifecycle/residual-state审计、public relative-link检查与
`mdbook build docs`。Kernel/xtask tests、DT dump/compile、build、QEMU、wrapper、rootfs、LTP、physical board
和final harness均Not Run；它们是Ready定义中的未来execution floor，不是本docs-only gate证据。

## Checkpoint 2A activation - 2026-07-23

**Status:** Activated；Checkpoint 2B保持Ready / Not Activated。

用户明确授权完成Stage 2前两个checkpoint；本记录只激活2A的frozen subset，不把Stage整体授权解释为
自动进入2B。Activation preflight重新读取R0 target/invariants、Ready definition、live Platform/build/QEMU/
bootstrap owner与两个ignored legacy DTB。QEMU 10.0.50 topology-only dump证明LA64 legacy blob与current
1-CPU/1-GiB provider除`/chosen/rng-seed`外一致；现有RV64 committed DTS则是可复现的旧4-CPU/128-MiB
provider snapshot，与current manifest drift。

Review将“保留现有RV64 DTS”解析并回写为保留文件identity及firmware-delivered/provider-derived baseline
角色，同时在2A一次性对齐current provider bytes；它不改变RV64 runtime FDT、QEMU execution、target、owner、
ABI、visible semantics或acceptance boundary。另在2B预审中发现Ready文本把R0的`template`/至少一次且可
多次placeholder误写为`args`/恰好一次；已在`implementation.md`按accepted target纠正，不递增R0，也不
激活2B。

## Checkpoint 2A execution and closure - 2026-07-23

**Status:** Closed；Stage 2 Active；Checkpoint 2B Ready / Not Activated。

**Change:** Platform `[dtb]`现在只保存workspace-relative committed `source`、typed delivery/authority与
provider-derived时必需的`provider = "qemu"`；parser和schema只接受当前两种闭合组合。RV64 committed
DTS已对齐current topology-only provider并保持firmware conformance-baseline角色；新增LA64 normative
DTS并保持embedded delivery。两份source都记录capture命令/QEMU版本并删除易变`rng-seed`。Normal build
不再调用`gen_qemu_cmd()`或`dumpdtb`，只用固定`dtc`把selected DTS原子发布到
`build/generated/device-tree/platform.dtb`；LA64 bootstrap只嵌入该build output。每次prebuild会先删除
上一snapshot的final/tmp，即使selected Platform无DT或source/dtc失败也不保留stale output。两个ignored
source-tree legacy DTB已删除；ordinary QEMU latest bytes未改变，refresh/bind/selection均未提前实现。

**Provider and build validation:** `qemu-system-riscv64`与`qemu-system-loongarch64`均为10.0.50
（v10.0.0-2143-gdf6fe2abf2），`dtc`为1.7.0。两个topology-only dump删除`/chosen/rng-seed`后，与本轮
committed/build DTB的sorted decompile逐字节相等。`dtc`仍报告RV64 numeric phandle与LA64
interrupt-provider warnings；它们来自provider snapshot且不阻塞compile，但本证据不声明完整DT correctness。
在临时隐藏root-level RV/LA test disk与RV rootfs、PATH前置exit-97 fake QEMU的环境中，非sandbox
`just build`分别完成RV64与LA64 release kernel build；fake QEMU未执行，两个build都生成build-local DTB。
随后无DT的VisionFive build成功，并确认fixed DTB/tmp均被清除。三次validation-only selection后原
`kconfig`逐字节恢复，SHA-256前后均为
`afc5faf697f2d7ef095c83d7412b7f2e7bb16db5b29afb30977ce6852c2a569f`；临时隐藏的runtime inputs全部恢复。
首次sandbox内RV64 build在DT compile后因lwext4 C build的`Bad system call`失败，不计为build通过证据；
随后按环境规则在非sandbox重跑成功。

**Static/schema/docs validation:** Latest bytes运行`just xtask-test`为32 passed / 0 failed；Python
`tomllib + jsonschema`验证全部6份Platform TOML，`jq empty conf/platforms/schema.jsonc`通过；normal-build
residual search只剩两份DTS provenance注释。`git diff --check`、public relative-link检查和
`mdbook build docs`通过，mdBook仅报告既有large search-index warning。两个DTS compile/decompile仍产生
上述provider warning；kernel guest boot、ordinary QEMU runtime、DT refresh、LTP、physical board和final
harness均Not Run。

**Review:** Independent latest-byte review为Apollyon 0。发现的新`device_tree.rs`命中根级无锚定
`build` ignore pattern；该Keter不通过越界修改`.gitignore`修复，而按2A frozen planned-new manifest用
`git add -f`纳入tracked commit，并以staged name-status复核。另一个Euclid是missing source或无DT Platform
可能留下上一snapshot；已改为每次prebuild先清除final/tmp，并由VisionFive build neutralize。RV64 baseline
alignment、DT owner/delivery、LA64 include、failure cleanup、普通QEMU不变与2B未激活均通过复核；dtc warning
为Safe并按实际边界记录。

**Result:** Checkpoint 2A Closed，没有命中Stage 2 target/owner/API/shared-contract/ABI/visible-semantics/
acceptance停止条件；Contract cutover仍为None，`BOOT-PROTOCOL-001` effective baseline与pending successor
不变。2B保持Ready / Not Activated，须在本checkpoint提交后单独记录activation。

## Checkpoint 2B activation - 2026-07-23

**Status:** Active；Checkpoint 2C Not Started / Not Authorized。

用户对Stage 2前两个checkpoint的明确授权在2A独立提交`d74b3235`后只激活2B。Activation preflight读取
2A latest commit、R0 target/invariants、已修正的Ready definition、live reference/resolver/kconfig/workspace/
Platform/QEMU owner与tracked target matrix。2B只建立dormant BuildPreset、selection与bind foundation：
production build继续以legacy bridge进入同一个`ResolvedSystemBuild` owner，普通QEMU继续使用当前
`--platform/--image`和tracked argv；Justfile、main、wrapper与tracked Platform均不进入本checkpoint。

Resolved subset仍以canonical `implementation.md`为唯一权威。若实现要求第二resolver、改写Stage 1
reference identity、让`CargoProfile`进入app/rootfs、让bind value进入snapshot，或提前暴露半成品CLI/
切换tracked QEMU runtime，立即停止2B。Contract cutover为None，2C不由本activation授权。

## Checkpoint 2B closure - 2026-07-23

**Status:** Closed；Stage 2 Active；Checkpoint 2C Ready / Not Activated。

**Change:** 新增strict `BuildPresetRef`、closed `BuildPreset`/`CargoProfile`与selection parser，六份
tracked preset覆盖五个SystemTarget并为RV64 pretest提供dev/release一对多；tracked default与ignored
local selection都只保存preset ref。Legacy `[build].profile`直接使用唯一`CargoProfile` enum；为保持
production build task latest bytes不变，`kconfig::Profile`只是带2C退出条件的owner-local re-export，不是
第二个类型或状态源。Legacy与dormant selection经同一个private `resolve_owned_system()`生成唯一owned
`ResolvedSystemBuild`；explicit selection在读取local/default前完成，local以目录项存在性为准，只有真正
缺席才回退default，dangling/unreadable state均失败。Platform parser增加dormant ordered bind declaration，
QEMU helper以`OsString`按declaration order逐token替换一个或多个`{{}}`，并拒绝invalid
declaration/value/path；helper只由unit fixture调用。

**Validation:** Latest bytes运行`just xtask-test`为46 passed / 0 failed。`jq empty`验证两份新schema；
Python `tomllib + jsonschema`验证example、六份concrete preset与tracked default selection。新Rust模块、
selection resolver与QEMU bind helper的定向`rustfmt --check`通过；
`git check-ignore -v conf/.selection.toml`命中精确`/conf/.selection.toml`规则。Source audit确认production仍
只有`tasks/build/mod.rs -> resolve_legacy_build()`一个caller；dormant resolver与bind helper只在定义/测试中
出现；`ResolvedSystemBuild`没有bind value；app/rootfs不导入`CargoProfile`；tracked Platform没有
`[[qemu.bind]]`。Justfile、main、build task、wrapper与tracked Platform相对`d74b3235`均无字节改动，live
`just --list`及xtask/build/QEMU help仍只暴露legacy surface。`git diff --check`、public relative-link检查与
`mdbook build docs`通过；mdBook只报告既有large search-index warning。Kernel build、QEMU/guest runtime、
rootfs、wrapper、LTP、physical board与final harness均Not Run。

**Review:** Independent latest-byte review最终为Apollyon 0 / Keter 0 / Euclid 0。Interim review发现
dangling local selection会把target `NotFound`误判为local文件缺席；已改为先以`symlink_metadata`线性化
目录项存在性，再读取内容，并补回归测试。Review还发现三处canonical lifecycle仍停留在旧Stage 2/2B
状态，已同步为2A-2B Closed / 2C Ready, Not Activated。`kconfig::Profile`经复核只是唯一
`CargoProfile`的带退出条件re-export，分类为Safe；single resolver/snapshot、production dormancy、schema、
bind token/path边界与latest lifecycle surfaces均通过。Reviewer全程只读，未编辑、暂存、提交或进入2C。

**Result:** Checkpoint 2B Closed，没有命中Stage 2 target/owner/public API/shared-contract/ABI/
visible-semantics/acceptance停止条件；Contract cutover仍为None，`BOOT-PROTOCOL-001` effective baseline与
pending successor不变。2C只达到Ready / Not Activated，本事务不会自动进入。

## Checkpoint 2C activation - 2026-07-23

**Status:** Active；Checkpoint 2D Ready / Not Activated。

用户对Stage 2最后两个checkpoint的明确授权在2B独立关闭并以`77d263be`提交后只激活2C。Activation
preflight重新读取R0 target/invariants、Stage 2 Ready definition与resolved manifest、tracking、register、
current transaction，以及live Justfile/xtask/config/Platform/wrapper/build-skill owner。2A/2B的DT输入、
single snapshot、dormant preset/selection/bind foundation与`BOOT-PROTOCOL-001`边界保持成立；没有发现
新的shared runtime contract或阻塞Stage 2的live design issue。

2C只执行atomic production CLI、QEMU、wrapper、cleanup与durable-surface cutover。若必须保留legacy
selection/QEMU入口或compatibility alias、让wrapper继续改写semantic config、让build/QEMU消费不同
snapshot，或需要改变target/owner/public API/shared contract/ABI/visible semantics/acceptance boundary，
立即停止。Contract cutover仍为None；2D不由本activation自动进入。

## Checkpoint 2C write-set expansion - 2026-07-23

**Status:** Approved；Checkpoint 2C仍为Active，2D仍为Ready / Not Activated。

Latest-byte residual audit发现两份tracked `scripts/qemu-virt-{rv64,la64}-dbg.just`仍直接保存并执行raw
QEMU argv，形成绕过shared selection/bind owner的第二入口；`conf/rootfs/visionfive2/README.md`仍发布2C
已经删除的`conf switch`与build `-k`接口。两类文件均在原frozen subset之外，因此implementer先停止，
上报扩展理由、范围、contract影响与验证计划，未提前编辑越界文件。

用户确认两份dbg入口已数月不用、属于遗留代码，不要求迁移，并批准直接更新VisionFive README。Resolved
manifest据此最小扩展为删除两份dbg文件、把README改为显式`visionfive2-rv64-release` preset流程；wrapper
仍在原subset内修复任何`build/` mutation之前的symlink fail-closed。该扩展只清除旧入口并同步durable
workflow文档，不改变target、owner、public API、shared contract、ABI、visible semantics或acceptance；R0与
Contract cutover None保持不变。验证增加tracked raw-QEMU/legacy CLI residual search、README命令source
audit、wrapper pre-mutation safety检查、live help/rejection与既有2C/2D floor。

## Checkpoint 2C closure - 2026-07-23

**Status:** Closed；Stage 2 Active；Checkpoint 2D Ready / Not Activated。

**Change:** Build/QEMU已切换到`SelectionArgs`与`ConfigLoader::resolve_selection` single resolver；legacy
`[build]`、`resolve_legacy_build`、`conf switch`、Platform name/aliases、tracked QEMU executable/path与
`--platform/--image`均删除。普通QEMU使用action-owned fixed architecture program、tracked ordered bind
templates与explicit host paths；Justfile、selection CLI、cleanup、两份pretest wrapper、Platform schema/
examples、build-system skill同步完成。wrapper在任何`build/` mutation前拒绝symlink，并使用runtime-local copy、
realpath inequality与`cp --remove-destination`保护master。获批扩展删除两份数月未使用的raw-QEMU dbg justfile，
并将VisionFive README改为显式`visionfive2-rv64-release` preset。

**Validation:** `just xtask-test`为50 passed / 0 failed；tracked platform/preset/default/schema matrix通过；
live `just --list`与build/qemu/conf/selection/clean help通过；RV64/LA64 preset及complete low-level tuple
`--show-bindings`通过；fake fixed QEMU exact argv通过（RV64 34 tokens、LA64 28 tokens，含debug与bind order）；
legacy `conf switch`、build `-k`、QEMU `--platform/--image`、`mrproper`、`xtask-clean`与`gendisk`均稳定拒绝；
physical Platform QEMU path稳定拒绝；两份wrapper `bash -n`与隔离`build -> external` symlink regression通过；
residual owner/path audit、`git diff --check`、relative-link audit与`mdbook build docs`通过（仅既有large
search-index warning）。`just fmt xtask --check`因standalone xtask不是root workspace member而失败；使用同一
`rustfmt.toml`对本checkpoint Rust files的`skip_children=true --check`通过。normal build、production RV64
wrapper及其真实rootfs/QEMU/guest shutdown属于2D floor，尚未运行。真实LA64 QEMU、physical board、LTP
全量与final harness也均Not Run，但按Ready定义明确不属于Stage 2 closure floor。

**Review:** Independent latest-byte review为Apollyon 0 / Keter 0 / Euclid 0 / Safe 0。Reviewer确认扩展
先写入authoritative manifest与本transaction，dbg删除没有tracked consumer，README与live preset一致，single
resolver/QEMU owner、bind token/path边界、cleanup保留`kconfig`/local selection、wrapper master safety与
legacy residual均闭合；Reviewer只读，未编辑、暂存、提交或进入2D。

**Result:** Checkpoint 2C Closed；没有命中target/owner/public API/shared-contract/ABI/visible-semantics/
acceptance停止条件；Contract cutover仍为None，`BOOT-PROTOCOL-001` effective baseline与pending successor不变。
Checkpoint 2D保持Ready / Not Activated，不因本closure自动进入。

## Checkpoint 2D activation - 2026-07-23

**Status:** Active；Stage 2仍为Active；Stage 3保持Outline。

用户对Stage 2最后两个checkpoint的明确授权在2C独立关闭并以`1daea8fa`提交后才单独激活2D。Activation
preflight读取2C committed diff、latest independent review、validation evidence、Stage 2 Ready definition/
full manifest、R0 target/invariants、register/current limitations、current transaction与live build/QEMU/
wrapper/cleanup owner。2C atomic cutover保持single resolver、Platform bind与wrapper master-safety边界；没有
新的target/owner/public API/shared-contract/ABI/visible-semantics/acceptance变化。

2D只执行Ready definition中的normal-build independence、selection/clean matrix、fake exact QEMU、real RV64
production wrapper、docs/status/residual与final review floor，并在finding只需Stage 2 full manifest内修复时继续。
Root `kconfig`、local selection、root-level legacy disks与final master均视为validation-only用户状态：执行前记录
身份/内容，必要的临时隐藏或重置后逐字节恢复，master只读。若缺master/sudo/host tool/runtime资源，2D保持
Active并准确记录Not Run；若命中Stage 2停止条件则停止。Contract cutover仍为None；不得运行或解析Stage 3
resolution gate。

## Checkpoint 2D write-set expansion - 2026-07-23

**Status:** Approved；Checkpoint 2D与Stage 2仍为Active；Stage 3保持Outline。

Latest-byte tracked residual audit发现`.github/workflows/ci.yml`仍通过`conf switch`依赖并改写interactive
selection，`.vscode/tasks.json`的两份QEMU task仍发布已删除的`--platform/--image`入口；两者均是live
automation/workflow consumer，且原frozen manifest未包含它们。Implementer据此先停止，没有提前修改越界文件，
并上报扩展理由、文件范围、contract影响和验证计划。

用户批准把两份文件纳入2D，且明确CI必须同步适配。Authoritative manifest先扩展为
`.github/workflows/ci.yml`与`.vscode/tasks.json`，随后才允许编辑consumer：CI改用显式RV64/LA64 release
preset，VS Code QEMU task改用对应显式preset和`kernel-image` bind，并保留现有debug与日志目的地。该扩展只
迁移2C cutover遗漏的workflow consumer，不改变target、owner、public API、shared contract、ABI、visible
semantics或acceptance boundary；R0、Contract cutover None与Stage 3边界不变。验证增加workflow syntax、CI
两条显式preset build、两份task的fake-QEMU exact invocation以及全tracked legacy CLI residual audit。

## Checkpoint 2D closure - 2026-07-23

**Status:** Closed；Stage 2 Closed；Stage 3保持Outline / Not Resolved。

**Change:** 2D完成2A-2C latest bytes的integration/production validation、consumer residual closure、
independent final review与lifecycle write-back。获批扩展把CI迁移为显式
`qemu-virt-{rv64,la64}-release` preset build，并把两份VS Code QEMU task迁移为对应显式preset、唯一
`kernel-image` bind、既有debug与日志目的地。没有修改current contract、register、其它RFC或Stage 3实现。

**Build/config validation:** `just clean`保留用户`kconfig`与初始缺席的local selection；`just defconfig`
只生成KernelConfig且不含legacy `[build]`，随后将用户文件逐字节恢复。Default/local set-show-clear、invalid
local fail-closed、explicit覆盖invalid local、complete/incomplete low-level tuple与physical-QEMU unsupported
matrix通过。Rootfs/runtime/legacy disk路径缺席且PATH前置必失败fake QEMU时，RV64/LA64 explicit pretest
preset normal build均成功，证明build不启动QEMU或读取runtime bind；两架构均从committed DTS产生build-local
DTB，source tree没有generated DTB。沙箱内首次lwext4 build因seccomp触发`Bad system call (159)`；相同构建
在沙箱外成功，归类为执行环境限制而非source failure。

**Workflow validation:** CI中的精确命令
`just build --preset qemu-virt-rv64-release && just build --preset qemu-virt-la64-release`在沙箱外原样通过；
RV64与LA64均完成release build。`actionlint`在host不可用；Ruby YAML parser与source assertion验证CI语法和
显式preset，JSONC结构解析与`jq` assertion验证两份VS Code命令。PATH前置`true` fake并以`strace execve`
捕获两份task，分别得到fixed `qemu-system-riscv64` / `qemu-system-loongarch64`、`-s -S`及
`-kernel build/anemone.elf`的exact argv；sandbox禁止ptrace，捕获在沙箱外完成且未启动真实QEMU。
Live workflow residual audit未发现`conf switch`、legacy build `-k`、QEMU `--platform/--image`或dbg launcher
consumer；Cred Merge旧implementation中的`conf switch`经用户确认属于已关闭RFC的历史材料，不迁移。

**Runtime validation:** Fake pretest QEMU exact argv继续为RV64 34 tokens、LA64 28 tokens，bind order、debug
tokens与三个host path精确；show-bindings、live help、legacy rejection和两份wrapper `bash -n`通过。真实
`./scripts/run-user-test-rv64.sh etc/final/images/sdcard-rv.img
build/system-target-stage2-rv64.log`退出0：wrapper使用显式preset与完整bind map，runtime test disk位于
`build/runtime/pretest-rv64/disk-x0.img`，guest打印`user-test: all competition tests finished.`与
`user-test: all tests finished, shutting down.`并正常关机。只读master前后SHA-256均为
`2f7e3529cee1f88fb88535c0dcb0b1a7ee463ebdb76131180623af0519a5e9fb`，master与runtime副本为不同regular-file
inode；用户`kconfig`最终SHA-256保持
`afc5faf697f2d7ef095c83d7412b7f2e7bb16db5b29afb30977ce6852c2a569f`，local selection恢复为初始缺席。
Final image缺少LTP executable，配置的signal/wait groups只报告skip，因此本次不声明完整LTP证据。

**Tests/docs/review:** `just xtask-test`为50 passed / 0 failed；tracked Platform/SystemTarget/BuildPreset与
default selection schema/load matrix通过；live help与全部legacy CLI rejection通过；wrapper syntax、
write-set、ignored-local、tracked residual、相对链接、状态残留、`git diff --check`与`mdbook build docs`
通过。Independent latest-byte reviewer结论为Apollyon 0 / Keter 0 / Euclid 0 / Safe 0；reviewer只读，
没有编辑、暂存、提交或运行Stage 3 gate。

**Not Run / Result:** 真实LA64 QEMU、physical board、完整LTP与final harness均Not Run，按Ready definition
不属于Stage 2 closure floor。2A-2D已分别关闭，Stage 2没有命中target/owner/public API/shared-contract/
ABI/visible-semantics/acceptance停止条件；Contract cutover仍为None，`BOOT-PROTOCOL-001` effective baseline
与pending successor不变。Stage 3保持Outline，本closure没有运行或解析`Stage 2 -> Stage 3
Implementation Resolution Gate`。

## Stage 2 -> Stage 3 Implementation Resolution - 2026-07-23

**Status:** Completed；Stage 3 Ready / Checkpoint 3A Not Activated。

本gate在Stage 2独立关闭后只读执行，重新读取Stage 2最终diff、2A/2D review与validation、live Platform
parser/schema、全部tracked Platform、QEMU/normal-build DT owner、两份QEMU DTS与两份现存VisionFive DTS、
register/current limitations、R0 target/invariants与current transaction。Stage 2的single resolver、normal-build
QEMU independence、RV64 firmware/provider-derived baseline和LA64 embedded/normative delivery均保持成立。

Preflight确认剩余工作属于同一个Platform/QEMU-DT owner-local闭包，不需要用多个checkpoint串联取证。Gate因此
只解析Checkpoint 3A：nested `qemu dt refresh`直接加载PlatformRef，共用单一
`dumpdtb -> compile/decompile -> canonicalize -> compare`管线，提供check drift专用exit classification与
provider-derived baseline原子写回；同时把VisionFive现存board DTS分类为physical firmware-derived
conformance baseline，并完成全部6份tracked Platform的authority/delivery矩阵。另一份未被live Platform引用的
VisionFive DTS不成为并列owner。

Provider枚举只增加`firmware` provenance分类；QEMU refresh capability仍只属于`provider = "qemu"`，LA64
normative source与VisionFive firmware source都不可被maintenance action改写。该解析不改变target、kernel runtime
FDT接受、root-mount ABI、public runtime API、shared contract、visible semantics或acceptance boundary；
Contract cutover仍为None，`BOOT-PROTOCOL-001` effective baseline与pending successor不变。Authoritative Ready
definition、validation floor、stop/recovery与完整manifest只位于RFC `implementation.md`。

**Resolution review / validation:** Apollyon 0 / Keter 0 / Euclid 0。运行`git diff --check`、public relative-link、
lifecycle/status与write-set文本审计，并运行`mdbook build docs`。Xtask tests、真实QEMU DT check、normal build、
physical board、LTP与final harness均Not Run；它们属于Checkpoint 3A execution floor，不是docs-only resolution
证据。Resolution不自动激活3A。
