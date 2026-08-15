# Nemophila 当前契约

**Contract ID：** `NEMOPHILA-RUNTIME-001`、`NEMOPHILA-HOST-001`、
`NEMOPHILA-WEAVE-001`、`NEMOPHILA-CLONE-001`、`NEMOPHILA-THREAD-EXIT-001`、
`NEMOPHILA-ARTIFACT-001`
**状态：** Active
**Owner：** Nemophila runtime；各 point / Host service / artifact source 的参与 owner 见下表
**参与领域：** SystemTarget / module build / task credentials / native syscall ABI / VFS file snapshot /
Core Wasm interpreter / task clone与exit / printk / procfs
**覆盖范围：** R0 trusted-good-module 的 artifact acquisition、transactional admission、published instance lifecycle、
typed weave、clone与thread-exit observer、value-only logging、management ABI 与只读诊断投影
**不覆盖：** untrusted module execution、执行配额或进度保证、force/automatic unload、source replacement/priority、
path-based load ABI、并发 writer 原子 snapshot、当前logging之外的新Host service、generic procfs dentry revocation或dynamic inode
materialization
**实现位置：** `anemone-abi/src/nemophila.rs`、`anemone-kernel/src/nemophila/`、
`anemone-kernel/src/fs/proc/nemophila/`、`anemone-kernel/src/task/api/{clone,exit}/`、
`scripts/xtask/src/{config, tasks/{build,module}}`、`nemophila/`、`anemone-rs/src/{sys,os}/anemone/nemophila.rs`
**依赖：** [`STM-TARGET-001`](../configuration/system-target.md#stm-target-001--systemtarget-是-bootdeploy-contract)、
[`BOOT-PROTOCOL-001`](../task/boot-protocol.md#boot-protocol-001--typed-initial-program-source统一收口到普通-vfs-exec)、
现有 VFS opened-file / positioned-read 与 kernel logging owner
**Pending Successor：** None
**最后核验：** 2026-08-15；`ANE-CHG-20260815-nemophila-task-lineage-auditor` Effective

本页从已经完成双架构 R0 acceptance 的 live implementation 提取后续模块、point、Host service与产品配置可共同依赖的
最小规则。完整历史 target、取舍与验证边界见
[Nemophila RFC R6](../../rfcs/nemophila/index.md)和
[closure transaction](../../devlog/transactions/2026-08-14-nemophila.md)。

## 状态与能力所有权

| 状态 / 能力 | 唯一 Owner | 其它参与方持有什么 | 行为用途 |
| --- | --- | --- | --- |
| Core Wasm parse / validation / translation / execution truth | `nemophila-wasm` | checked module / instance API | admission与guest execution |
| published instance、identity、binding、in-flight、poison与retirement | Nemophila runtime | typed point capability或immutable value snapshot | lifecycle与callback admission |
| point identity、signature、binding policy、context lowering与call site | provider subsystem | typed `PointSpec` / descriptor | subsystem-owned extension seam |
| module interface schema | versioned WIT package | generated/private consumer view | logical import / export contract |
| module build recipe、fresh candidate与ordinary export | module build owner | system build收到同一invocation immutable bytes | artifact materialization |
| ordered required embedded identity selection | SystemTarget | resolved/generated immutable projection | product boot selection |
| effective `CAP_SYS_MODULE` truth | task credentials | operation-local authorization result | management admission |
| supplied source file与positioned-read behavior | VFS / opened file | operation-local reference与owned bytes | source snapshot acquisition |
| log filter、record、ring与console projection | kernel logging | borrowed validated UTF-8 value | Host logging submission |
| `/proc/nemophila` content | Nemophila runtime snapshot | procfs-owned immutable text/enumeration snapshot | read-only diagnosis |

## NEMOPHILA-RUNTIME-001 — Runtime唯一拥有published instance lifecycle

**规则：** Nemophila runtime是published instance、opaque nonzero `u64` identity、binding、in-flight、poison与retirement的唯一
owner。每次load独占完整interpreter entity；checked admission、start rejection、narrow linking、typed module-side `load`和
registration都在unpublished transaction内完成，只有commit同时发布identity、instance与bindings。任一pre-commit失败完整
rollback；identity单调分配且不复用。

userspace只有一个closed tagged-source load syscall与一个独立try-unload syscall。两者在copy user memory、访问fd或修改runtime
前检查current effective `CAP_SYS_MODULE`。load success返回identity；try-unload只接受nonzero identity与zero flags。zero
in-flight的live或poisoned instance可不可逆retire；存在admitted、waiting或executing callback时返回`EBUSY`且无副作用。

每个instance的guest entry串行。module-caused callback trap在释放触发slot前把authoritative lifecycle置为poisoned，随后取消该
instance尚未进入guest的admitted callbacks；其它fanout instance继续。poison不自动恢复或释放bindings/resources，直到显式
successful retirement。procfs得到的origin/lifecycle/in-flight都是同一owner临界区内复制的诊断值，不能驱动runtime决策。

management failure classes保持closed：authorization为`EPERM`；user copy为`EFAULT`；wire/syntax/zero identity/nonzero flags、
reserved或non-regular source为`EINVAL`；缺失embedded identity或nonzero published identity为`ENOENT`；invalid/unreadable/
`O_PATH` fd为`EBADF`；artifact超限为`EFBIG`；source I/O保留对应errno；interpreter/admission/module-side load拒绝为`ENOEXEC`；
identity/transaction counter耗尽为`EOVERFLOW`；in-flight unload为`EBUSY`。

**违反表现：** procfs、catalog、task point或management caller保存并驱动另一份live/loaded/poisoned truth；load失败后残留
publication/reservation；callback并行进入同一instance；busy失败撤销binding；poison自动恢复；retired identity重新指向新instance；
缺少capability的caller能通过后续错误观察source/runtime。

**验证 / Enforcement：** runtime owner KUnit、wire/layout assertions、authorization/source/lifecycle focused guest、
RV64/LA64 R0 wrapper与source/owner audit；cutover证据见closure transaction。

## NEMOPHILA-HOST-001 — Host capability不转移service truth

**规则：** Host import分为provider-owned typed extension registration与kernel-owned service submission。当前唯一service是
value-only logging。结构化`write`完整投影kernel的八个severity；既有debug/info/warning/error保持原Core Wasm discriminant，新增
emergency/alert/critical/notice追加在后。Host验证level、guest memory range与UTF-8后，把borrowed value交给现有kernel logging
owner；filter、record bound、truncation、ring和presentation仍由logging决定。

raw `print/println`只提交console-only fragment或line，并明确沿用kernel `kprint!/kprintln!`语义：不伪造成结构化record，
不携带severity，也不写ring。SDK同名格式化macro只在guest内形成owned string，再走同一value-only Host边界。两类提交都不返回
resource handle、不建立跨调用borrow，不决定module publication、callback结果、poison或unload；已经提交的load diagnostic不因
transaction rollback而撤销。

无效guest level/pointer/length/memory/UTF-8只形成contained module failure或trap，不能panic kernel。WIT/API只描述logical value
boundary，不拥有provider availability、binding policy、service policy或runtime lifecycle。

**违反表现：** logging Host保存guest pointer、返回kernel object、让结构化write绕过现有logging policy、把raw fragment伪装成
record、以日志成功/失败驱动runtime状态，或把provider registration当作generic service/resource registry。

**验证 / Enforcement：** 八级write与raw print/println lowering/invalid-input KUnit、真实module load/callback logging与source audit。

## NEMOPHILA-WEAVE-001 — Point声明policy，runtime托管binding

**规则：** provider subsystem唯一拥有point identity、typed context/parameters、binding policy、WIT lowering与semantic call site；
Nemophila提供generic typed declaration、immutable provider catalog、transaction-local registration/reservation、publication、cohort
selection、invocation ownership与lifecycle。descriptor地址、link order或dynamic registration都不是point identity truth。

registration只在module-side `load` call window内有效；同一instance/point至多一次。provider unavailable、already registered与
exclusive conflict作为typed result返回module，module决定是否拒绝整个load。successful reservation与instance一起publish；load
error/trap完整rollback。Fanout按一次受保护selection形成cohort，callback trap隔离到对应instance；poisoned exclusive binding仍
占位，直到successful retirement。

WIT world只组合已经存在的真实module shape：无callback lifecycle、clone-only以及clone+thread-exit。artifact不因SDK固定export
而携带未注册point的dummy callback；同一instance可在一个load transaction中注册多个typed point，任一失败仍由该transaction
整体rollback。

**违反表现：** provider持有runtime locks/private instance，runtime解释provider context，registration绕过transaction直接publish，
module获得raw function/table/token，point policy由SystemTarget或catalog order决定，或poisoned exclusive occupancy被隐式替换。

**验证 / Enforcement：** provider/catalog/registration/cohort/concurrency/poison/retirement KUnit，lifecycle-only、clone-only与
clone+thread-exit fresh module build，以及真实multi-point guest证据。

## NEMOPHILA-CLONE-001 — Clone observer只接收committed TID snapshot

**规则：** task owner在共同`kernel_clone()`成功路径中、child已经publication并enqueue之后、`CLONE_VFORK` wait或creator return之前，
向唯一typed `CloneObserver` point同步提交creator与child的`u32` TID value snapshot。creator按实际caller取得；`CLONE_PARENT`只改变
parent relation，不改写observer creator。publication/enqueue guard与scheduler-private state不得跨入callback。

observer normal return、trap、poison或缺少binding都不能撤销child、改变clone/clone3返回值、reparent、reap或vfork completion；
callback失败由Nemophila instance lifecycle containment，task owner不读取runtime私有状态。

**违反表现：** 失败clone触发observer；notify发生在child可运行前；callback持有Task/guard/lock；`CLONE_PARENT`报告relation parent
而非creator；observer结果反向改变clone语义。

**验证 / Enforcement：** call-order/source proof，fork与raw clone3双架构guest callback/log evidence，以及runtime trap/fanout proof。

## NEMOPHILA-THREAD-EXIT-001 — Thread-exit observer只接收入口值快照

**规则：** task exit owner在每个user task进入不可返回的`kernel_exit()`路径后、任何futex/file/timer/topology或ThreadGroup
cleanup开始前，从开中断、可睡眠且未持task-private guard的窗口向唯一typed `ThreadExitObserver` point同步提交TID与本次
`ExitCode`值快照。normal exit只携带八位status，signal exit只携带signal number。

该事件只表示thread-exit-begin；它不表示ThreadGroup已经`Exited`、child已经waitable/reapable、task cleanup完成或scheduler
Zombie已经发布。observer normal return、trap、poison或缺少binding都不能改变退出原因、cleanup顺序、topology detach、parent
notification或zombie handoff；task owner不读取runtime私有状态。

**违反表现：** callback发生在持有task/ThreadGroup/topology guard时，携带Task或handle而非值快照，把observer结果写回exit code，
或module的关联表反向成为task/thread-group lifecycle truth。

**验证 / Enforcement：** `kernel_exit()` call-window与owner source proof，provider/catalog KUnit，以及RV64
`task-lineage-auditor`对真实clone到exit(0/7)的matched callback/log证据。依赖[`TASK-LIFE`](../task/thread-group-lifecycle.md)，
但不改变其terminal owner与publication语义。

## NEMOPHILA-ARTIFACT-001 — 两种来源汇入同一immutable admission

**规则：** versioned WIT package是logical interface唯一source；module build owner唯一拥有manifest/toolchain/fresh candidate与ordinary
export。SystemTarget只保存ordered duplicate-free required embedded identities；system build按resolved order调用module build owner，
立即核对ordinary export并消费同一invocation返回的immutable bytes，将其固定到本次system build私有snapshot后，在KernelConfig
size上限内生成只含identity/order/immutable bytes的kernel input。空selection产生空catalog；build不延后重读可替换stable
export、不复制module recipe、不建立mtime/provenance sidecar或loaded registry。

boot在provider catalog/global runtime可用、rootfs与KUnit结束且initial userspace尚未开始时按序load required embedded bytes；每个
失败transaction先rollback，再记录identity/ordinal/phase并boot-fatal。boot authority来自resolved SystemTarget，不伪装task或
`CAP_SYS_MODULE` request；order不是source/binding priority。

management embedded source按catalog identity复制bytes。supplied source只接受readable non-`O_PATH` regular fd，以positioned reads从
offset 0到首次EOF形成KernelConfig-bounded owned snapshot，不改变shared cursor；snapshot完成后、interpreter admission前释放file
reference，runtime不保存fd/inode/path/namespace。之后的write/truncate/rename/unlink不影响本次load；复制期间并发writer不承诺
linearizable bytes，但不得破坏memory/lifecycle safety。

两种source都进入同一个`load_and_publish` owner并产生相同runtime entity。`nemophila-wasm`唯一拥有integer-only Core Wasm
validation/execution；runtime在execution前拒绝malformed/unsupported input、start section、unknown Host import和缺失/类型错误的
required entry。WIT metadata、精确import/export集合与custom-section allowlist不成为第二份admission truth。RV64与LA64从同一
source revision分别fresh build，只有content hash一致的artifact bytes构成same-artifact acceptance。

**违反表现：** SystemTarget保存recipe或loaded truth；system build使用stale/fallback artifact；boot失败继续initial userspace；
embedded/supplied走不同runtime；supplied load保留pathname/file或改变cursor；source mutation改变已完成load；WIT metadata复制
interpreter validator；不同架构消费内容不同的artifact。

**验证 / Enforcement：** xtask config/module/generated-input tests、boot success/negative双架构oracles、supplied-fd focused cases、
interpreter/module regression、RV64/LA64 artifact SHA-256一致性与R0 lifecycle wrapper。

## 已知相邻边界

retirement会撤销Nemophila backend mapping，且cached positive inode在重新open时重验runtime membership。generic procfs/VFS
尚未提供dynamic semantic identity的并发唯一inode与回收协议；Nemophila不建立private inode registry。并发首次lookup同一live
identity的安全性与generic inode-cache lifecycle由
[`ANE-20260815-PROCFS-DYNAMIC-INODE-MATERIALIZATION`](../../register/open-issues.md#ane-20260815-procfs-dynamic-inode-materialization)
跟踪。generic VFS在lookup与positive dentry materialization之间的revocation也仍未形成全局线性化保证，由
[`ANE-20260809-VFS-DYNAMIC-POSITIVE-DENTRY-REVOCATION`](../../register/open-issues.md#ane-20260809-vfs-dynamic-positive-dentry-revocation)
继续跟踪，不在Nemophila内维护private freshness truth。
