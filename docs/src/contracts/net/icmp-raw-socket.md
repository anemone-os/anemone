# IPv4 ICMP Raw Socket 当前契约

**Contract IDs：** `NET-ICMP-RAW-INGRESS-001`、`NET-ICMP-RAW-ENDPOINT-001`、`NET-ICMP-RAW-TRANSACTION-001`
**状态：** Active
**Owner：** interface/IP admission、domain Stack ICMP raw owner、initial-domain control plane与kernel raw Socket分别拥有其局部state；本页拥有它们之间的raw packet协议
**参与领域：** IPv4 / ICMP / protocol Stack / Socket / VirtIO frame path
**覆盖范围：** post-admission raw observation、bounded Endpoint/fanout/lifecycle，以及non-`IP_HDRINCL` TX与detached RX transaction
**不覆盖：** arbitrary protocol、`IP_HDRINCL`、fragment/reassembly、broadcast/multicast、error queue、production Echo responder或message ABI
**实现位置：** `anemone-kernel/crates/anemone-net-api/src/icmp_raw.rs`、`anemone-kernel/crates/anemone-smoltcp-stack/src/icmp_raw/`、`anemone-kernel/src/net/icmp_raw.rs`、`anemone-kernel/src/fs/socket/icmp_raw/`
**依赖：** `NET-PROTOCOL-BOUNDARY-001`、`NET-SOCKET-WAIT-001`、`NET-CONTROL-PLANE-001`、`NET-FRAME-OWN-001`、`NET-FRAME-PROGRESS-001`、`NET-STACK-PUMP-001`、`OPENED-DESC-001..003`
**Pending Successor：** None
**最后核验：** 2026-08-03

## 状态与能力所有权

| 状态 / 能力 | 唯一 Owner | 跨owner handoff |
| --- | --- | --- |
| local IPv4 destination admission | concrete interface/IP owner | callback-scoped admitted original datagram与destination classification |
| route/source/interface selection | initial-domain IPv4 control plane | operation-local immutable selection |
| Endpoint identity/lifecycle、local/peer/filter、bounded storage与fanout/drop | domain Stack ICMP raw owner | opaque operation capability、typed outcome、facts与invalidation |
| TTL/TOS与Linux blocking/readiness policy | kernel ICMP raw Socket family | per-send immutable option snapshot、current facts |
| user copy、sockaddr/flags/errno | Socket ABI adapter | normalized request/cursor与typed result |
| RX detached packet / TX admitted packet | Endpoint owner；handoff后operation-local kernel或Stack transaction | single-owner byte capability |

## NET-ICMP-RAW-INGRESS-001 — Local admission先于非独占raw delivery

**规则：** 只有已经通过对应interface IPv4 parse/checksum与local-destination admission的packet可以进入ICMP raw
matching。destination admission truth只由interface/IP owner拥有；outer Stack、raw Endpoint与kernel Socket不得复制
assigned-address、broadcast/multicast或destination predicate。handoff保留IPv4 `total_len`内original bytes与
interface-owned destination classification，不重建TOS、ID、flags或IP options，也不延长driver frame lifetime。

Stack raw owner在post-admission handoff后独占ICMP protocol、R0 unicast/unfragmented policy、association/filter与fanout。
每个matching Endpoint取得独立计费的detached delivery；一个consumer full只丢自己的delivery，不能抑制普通ICMP处理、
generic smoltcp raw语义或其它Endpoint。fragment不进入R0 raw queue，ICMP body/checksum/type仍保持raw-visible。

**失败与cleanup：** malformed/foreign-destination/broadcast/multicast/fragment在相应owner拒绝，不发布raw readiness。
fanout allocation/capacity failure保持per-Endpoint drop，原packet继续由ordinary ICMP path拥有；callback返回后不得保留
borrowed frame或smoltcp representation。

**违反表现：** pre-admission raw delivery；第二份destination table；header re-emit替代original bytes；raw handled抑制
ordinary ICMP；一个full Endpoint终止fanout；或fragment静默进入raw queue。

**验证 / Enforcement：** original-byte vectors覆盖TOS/ID/flags/options、wrong destination与unsupported destination、
fragment rejection、multiple consumer/full isolation、ordinary ICMP coexistence、source audit及双架构真实ingress/ping。

**最初及当前来源：** [IPv4 ICMP Raw Socket RFC R0](../../rfcs/icmp-raw-socket/invariants.md#net-icmp-raw-ingress-001--local-admission先于非独占raw-delivery)的`ICMP-RAW-CUTOVER`。

## NET-ICMP-RAW-ENDPOINT-001 — Association、filter、fanout与retire由Stack raw owner统一拥有

**规则：** domain Stack raw owner唯一拥有monotonic Endpoint identity、semantic lifecycle、local/peer association、
ICMP filter、per-Endpoint bounded RX/TX packet/byte storage、matching fanout、drop accounting、capacity facts与retire。
kernel只持role-scoped non-blocking operation capability与opaque identity，不缓存association/filter/queue truth，不取得
private engine、registry或lock。

create preparation在fd publication前可以rollback；publication后只有opened-description semantic final release触发raw
source撤销并移交non-blocking Endpoint retire。dup/fork/non-final close、Rust `Drop`、temporary reference与raw fd number均
不能推进retire。Stack先withdraw active identity与owner-local resources；late/repeated notification、旧identity或延迟
cleanup不得命中新generation或恢复association。

bind/connect/disconnect/filter mutation在单一owner transaction中commit；失败不得部分改写既有association/filter。
每个matching Endpoint独立charge，drop counter只服务诊断，不反向决定readiness或lifecycle。orderly network shutdown先
关闭新admission，再由owner-local terminal path处理现有资源。

**违反表现：** kernel和Stack各存local/peer/filter；按fd查找Endpoint；关闭一个dup提前retire；full consumer阻塞整个
fanout；Endpoint持Task/File/waiter；old cleanup命中新identity；或final release等待worker。

**验证 / Enforcement：** create/rollback/retire、bind/connect/disconnect、multiple consumer/filter/full/drop isolation、
dup/fork/CLOEXEC/final close、late hint/stale generation、bounded resource与network shutdown proof。

**最初及当前来源：** [IPv4 ICMP Raw Socket RFC R0](../../rfcs/icmp-raw-socket/invariants.md#net-icmp-raw-endpoint-001--associationfilterfanout与retire由stack-raw-owner统一拥有)的`ICMP-RAW-CUTOVER`。

## NET-ICMP-RAW-TRANSACTION-001 — TX/RX各有唯一packet handoff与commit boundary

**TX规则：** user buffer只包含ICMP message。Socket ABI owner先完成bounded user copy与destination normalization；control
plane唯一选择route/source/interface；kernel raw family提供本次immutable TTL/TOS snapshot；Stack raw owner形成无IP
option、非分片IPv4 packet并在success前完成Endpoint admission commit。ICMP checksum由用户形成，kernel不修补或解释
message body。message超过selected interface MTU减20-byte IPv4 header返回`EMSGSIZE`；zero-length message仍形成真实
IPv4 transaction。commit后ordinary provider loss不反向改写send result。

**RX规则：** Endpoint把队首完整original IPv4 datagram原子detach给operation-local kernel transaction。detach前Endpoint
独占packet，detach后kernel独占；short buffer复制prefix但消费完整packet，`MSG_TRUNC`返回完整packet length，`MSG_PEEK`
在success/short/fault后均不消费。zero-length receive仍等待一个packet；ordinary success返回0并消费，带`MSG_TRUNC`
返回完整长度。non-peek copy fault可以丢弃已detach packet，但不requeue、重排或恢复readiness。

send的association、selection、option snapshot、copy与admission各有明确owner；blocking retry保留同一operation-local
destination与TTL/TOS snapshot，不跨sleep持Stack private phase。RX peer projection来自detached packet source，不写回
association。packet在kernel transaction、Endpoint、private engine、provider与ingress之间每次只有一个访问owner。

**失败与cleanup：** copy、address、route/source、MTU与capacity failure在对应commit前保留或释放当前owner资源，不留下
partial packet/association。retire与concurrent receive/send按Endpoint identity隔离；normal capacity不足可恢复，不panic、
busy-spin或用unbounded queue吸收。

**违反表现：** full-IP private TX buffer暴露为UAPI；kernel/Stack同时持mutable packet；无route仍success；provider failure
追溯改写send；short receive只消费prefix；peek改变queue；copy fault requeue；或blocking retry重读mutable option/peer。

**验证 / Enforcement：** owner-local TX/RX/header/capacity/retry tests、RV64/LA64 focused zero/short/peek/`MSG_TRUNC`/fault/
MTU/TTL oracle、BusyBox gateway ping与normal provider path audit。

**最初及当前来源：** [IPv4 ICMP Raw Socket RFC R0](../../rfcs/icmp-raw-socket/invariants.md#net-icmp-raw-transaction-001--txrx各有唯一packet-handoff与commit-boundary)的`ICMP-RAW-CUTOVER`。

## 当前接受边界

- 唯一tuple为具备effective `CAP_NET_RAW`的`socket(AF_INET, SOCK_RAW, IPPROTO_ICMP)`；只覆盖unicast、未分片IPv4 ICMP，TX由kernel/Stack形成IPv4 header，RX返回original datagram。
- closure evidence覆盖RV64/LA64 focused 10/10、BusyBox gateway ping 1/1、glibc/musl curated Socket LTP 6/6、owner-local/host proof与orderly shutdown。
- guest尚未把真实TX saturation、blocking retry与capacity recovery串成单个production-path case；owner-local proof已分别覆盖该组合，作为非阻断后续增强，不削弱上述current transaction rule。
- 公网、DNS、peer ping Anemone、physical hardware、`smp > 1`、其它NIC、full network LTP与final harness均Not Run。
