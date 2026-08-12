#import "../components/figure.typ": code-block, report-figure

= 网络栈

初赛结束时，网络仍然是 Anemone 计划中的下一项能力。进入决赛阶段后，我们完成了从网络设备驱动、IPv4 控制面、协议栈，到 Socket和只读 Netlink 诊断面的整条路径。现在，Anemone 已经可以运行 `ping`，执行 `git clone`，使用包管理器完成 `apt install`，也可以直接运行 `ip link/address/route show` 和 `ss -tan`等工具查看系统中的链路、地址与路由，我们的网络能力i经进入了可用状态。

== Socket 能力全景

Anemone 的通用 Socket 前端负责接收系统调用、处理用户内存与 Linux ABI，再把操作分派给具体协议族。前端使用一组静态操作表描述每种 Socket 的实际能力。数据面按照字节流（byte stream）、数据报（datagram）和有边界的可靠报文（seqpacket）三种形状组织，其余操作则按协议族是否真正支持来安装。

#code-block(
  ```rust
  pub(super) enum SocketIoOps {
      ByteStream {
          socket_type: SocketType,
          send: SocketSendOp,
          receive: SocketReceiveOp,
      },
      Datagram {
          socket_type: SocketType,
          send: SocketSendOp,
          receive: SocketReceiveOp,
      },
      Seqpacket {
          socket_type: SocketType,
          send: SocketSendOp,
          send_wait: SocketSendWaitOp,
          receive: SocketReceiveOp,
      },
  }
  ```.text,
  caption: [`SocketIoOps` 区分 stream、datagram 与 seqpacket 三种数据面能力（节选）],
  lang: "rust",
)

#code-block(
  ```rust
  pub(super) struct SocketOps {
      pub io: SocketIoOps,
      pub create: Option<fn() -> Result<SocketPreparation, SysError>>,
      pub create_pair: Option<fn() -> Result<SocketPairPreparation, SysError>>,
      pub bind:
          Option<fn(&AnyOpaque, SocketAddress) -> Result<(), SocketBindError>>,
      pub listen:
          Option<fn(&AnyOpaque, i32) -> Result<(), SocketListenError>>,
      pub connect:
          Option<fn(&AnyOpaque, SocketAddress) -> Result<(), SocketConnectError>>,
      pub accept:
          Option<fn(&AnyOpaque) -> Result<SocketAcceptItem, SocketAcceptError>>,
      pub shutdown:
          Option<fn(&AnyOpaque, SocketShutdown) -> Result<(), SocketShutdownError>>,
      /* 地址查询、option、poll 与 final release 见下段。 */
  }
  ```.text,
  caption: [`SocketOps` 静态装配创建、连接与连接生命周期操作（节选）],
  lang: "rust",
)

#code-block(
  ```rust
  // SocketOps 的查询、option、poll 与 release 字段（续）
      pub local_address:
          Option<fn(&AnyOpaque, &mut dyn SocketAddressSink)
              -> Result<(), SocketQueryError>>,
      pub peer_address:
          Option<fn(&AnyOpaque, &mut dyn SocketAddressSink)
              -> Result<(), SocketQueryError>>,
      pub accepting:
          fn(&AnyOpaque) -> Result<bool, SocketQueryError>,
      pub query_option:
          Option<fn(&AnyOpaque, SocketOptionQuery)
              -> Result<SocketOptionValue, SocketOptionError>>,
      pub mutate_option:
          Option<fn(&AnyOpaque, SocketOptionMutation)
              -> Result<(), SocketOptionError>>,
      pub poll:
          for<'a> fn(&AnyOpaque, &PollRequest<'a>)
              -> Result<PollRegisterResult, SysError>,
      pub final_release:
          fn(&AnyOpaque, SocketReleaseReason),
  ```.text,
  caption: [`SocketOps` 继续装配查询、option、readiness 与最终释放能力（节选，续）],
  lang: "rust",
)

我们用这套虚表同时承载 IPv4 TCP、UDP、ICMP raw、Unix stream/seqpacket 和 Netlink等协议族，将这些不同的后端能力统一为表现为文件的Socket。各协议族继续拥有自己的连接、报文、命名空间和错误语义。这样，一条系统调用可以复用公共的 Linux ABI 与用户内存处理，而不会迫使 TCP、UDP、Unix Socket 和 Netlink 共享一份含义模糊的内部状态。

== 从调研到四层架构

在设计网络栈之前，我们广泛调研了往届参赛内核。几乎所有实现都选择把 smoltcp（一个广泛使用的嵌入式网络协议栈 crate） 的对象与语义直接嵌入内核：Socket 对象保存 smoltcp 对象句柄，系统调用路径读取 smoltcp 状态，并据此计算 Linux 可见的 readiness 和错误。这条路线可以很快建立基本连通性，代码量也较少；但是，随着 bind、listen、accept、非阻塞 I/O、poll/epoll、close 和诊断能力逐步增加，具体后端的对象模型显然会扩散到越来越多的内核模块。

所以我们的选择是多建立一道边界。smoltcp 在 Anemone 中是协议引擎后端，它的对象句柄（handle）、Socket 集合、缓冲区和状态表示全部留在协议栈实现内部。内核通过规范化语义访问协议能力，并自行拥有 Linux Socket 的 fd、等待、readiness、errno 和生命周期。这项选择增加了早期设计与适配工作，也换来了清晰得多的对象关系。

我们把唯一负责保存和改变某项状态的模块称为状态属主（owner）。在此基础上，我们把跨模块的对象隔离原则称为 *object fence*：属主的内部对象停留在自己的边界内，跨层只传递规范化的值、不透明标识、一次性事实快照、重新检查通知或受限操作能力。接收方可以请求操作、观察事实和重新检查，却不能沿着一个对象句柄穿透到另一层的私有状态机。

#report-figure(
  image("../assets/network-stack.png", width: 96%),
  caption: [Anemone 网络栈的四层架构。数据包、操作命令、要求重新读取事实的事件，以及读取时刻的事实快照通过不同边界传递，各层只拥有自己的对象与状态。],
)

四层分别承担以下职责：

#figure(
  table(
    columns: (2.8cm, 3.3cm, 7.7cm),
    align: (center, center, left),
    inset: 6pt,
    stroke: 0.7pt,
    [*层次*], [*实现*], [*职责与 object fence*],
    [内核内部网络子系统],
    [`device/net`、`kernel::net`、Socket 前端],
    [拥有设备接入、内核逻辑网络接口、IPv4 路由与源地址选择、fd、等待和 Linux ABI；只使用规范化协议事实，不接触 smoltcp 对象句柄。],

    [Net API],
    [`anemone-net-api`],
    [定义帧交接、命令、重新检查通知、事实快照和受限操作能力；类型中不出现内核对象、smoltcp 对象或网卡 DMA/队列描述符编号。],

    [协议栈],
    [`anemone-smoltcp-stack`],
    [拥有协议接口映射、网络端点（Endpoint）、TCP/UDP/ICMP 状态与协议队列，并按单轮预算推进；不持有 fd、任务、内核等待关系或 Linux errno。],

    [协议栈后端],
    [`anemone-smoltcp`],
    [执行具体 TCP/IP 协议机制；其网络接口、Socket 集合、对象句柄与缓冲区表示不越过协议栈层。],
  ),
  caption: [四层网络架构的状态所有权],
  kind: table,
)

这四层并非简单的逐层转发关系。`anemone-net-api` 是内核和协议栈共同依赖的语义接口；`anemone-smoltcp-stack` 同时依赖 Net API 和协议引擎后端；smoltcp 本身不依赖任何 Anemone 模块。编译依赖的方向保证底层无法看到内核对象，Rust 的可见性规则与受限操作类型继续约束运行时能够交换的内容。

=== Command、Event 与 Snapshot

Net API 的上边界围绕三类交互组织。它们把“请求协议操作”“得知事实可能改变”和“读取当前事实”分开，使通知不会悄悄变成第二份协议状态。

#figure(
  table(
    columns: (2.3cm, 4.5cm, 7.0cm),
    align: (center, left, left),
    inset: 6pt,
    stroke: 0.7pt,
    [*交互语义*], [*当前代码中的形态*], [*边界含义*],
    [Command（命令）],
    [`create`、`bind`、`connect`、`send`、`receive`、`release` 等方法],
    [请求一次有界、非阻塞的协议操作；若产生待发送工作，则通知网络工作线程继续处理，不持有任务，也不在内部等待。],

    [Event（事件）],
    [TCP、UDP、ICMP 各自的端点失效通知],
    [只表示“相关事实可能变化，请重新读取”；可以合并，不携带 readiness 位图、错误码或连接阶段。],

    [Snapshot（快照）],
    [各协议的端点事实、接口事实与诊断记录],
    [给出读取时刻的一次性事实；内核据此计算 readiness、errno 和诊断输出，收到事件后重新取得快照。],
  ),
  caption: [Net API 的 Command、Event 与 Snapshot 三类核心交互],
  kind: table,
)

这三者是我们的核心设计模型。不过，源码没有建立一个包罗所有协议的 `Command`、`Event` 或 `Snapshot` 总枚举。代码形状上，三类语义分别为表现为窄方法和 TCP、UDP、ICMP 各自的具体类型，新增协议无需不断扩大一组公共大枚举。它们共享边界规则：命令不阻塞，事件只要求重新检查，快照只描述读取时刻的事实。所有这些能力都是非阻塞的，协议栈不负责同步能力，这些东西归内核内部处理。这样，我们能通过非阻塞的原语自由组合，配上内核的等待原语，实现各种阻塞和非阻塞的 Linux ABI。

== 帧路径：资源身份停留在设备侧

我们用协议栈和网卡驱动的下边界作为一个例子讲讲 Object fence 这个设计原则。简单来说，网卡驱动拥有收发帧的底层存储、队列描述符、DMA 映射、可用队列槽数量和完成回收状态等设备内部状态。协议栈只需要“取得一帧并消费”或“取得一个发送槽并填充”的能力。我们使用只能移动一次的 token 表达这次临时所有权，帧内存的借用期被限制在回调函数内，协议栈无法把设备侧存储保存到自己的对象中。

#code-block(
  ```rust
  pub trait RxToken {
      fn consume<R, F>(self, f: F) -> R
      where
          F: FnOnce(&[u8]) -> R;
  }

  pub trait TxToken {
      fn capacity(&self) -> usize;

      fn consume<R, F>(
          self,
          len: usize,
          f: F,
      ) -> Result<R, FrameSizeError>
      where
          F: FnOnce(&mut [u8]) -> R;
  }

  pub enum ReceiveOutcome<R, T> {
      Ready { rx: R, tx: T },
      Empty,
      TransmitExhausted,
      LinkUnavailable,
  }

  pub enum TransmitOutcome<T> {
      Ready(T),
      Exhausted,
      LinkUnavailable,
  }
  ```.text,
  caption: [一次性 RX/TX token 与明确的结果枚举共同描述帧资源交接（节选）],
  lang: "rust",
)

#code-block(
  ```rust
  pub trait FrameProvider {
      type RxToken<'a>: RxToken where Self: 'a;
      type TxToken<'a>: TxToken where Self: 'a;

      fn receive(&mut self, now: Instant)
          -> ReceiveOutcome<Self::RxToken<'_>, Self::TxToken<'_>>;
      fn transmit(&mut self, now: Instant)
          -> TransmitOutcome<Self::TxToken<'_>>;
      fn capabilities(&self) -> FrameCapabilities;
      fn link_state(&self) -> LinkState;
  }
  ```.text,
  caption: [`FrameProvider` 只向协议栈开放取得帧、发送槽和链路事实的受限接口（节选，续）],
  lang: "rust",
)

`ReceiveOutcome` 把接收帧与一个可能用于立即回复的发送槽一起交给协议引擎，这是因为 smoltcp 处理一份输入时可能立刻产生响应。设备队列为空、发送槽耗尽和链路不可用都是正常结果；协议栈不会为这些情况触发内核异常，也不会把它们折叠成同一个模糊的失败。

协议推进由 `Stack::pump` 完成；这里的 pump 指在给定预算内让协议状态机向前运行一轮。`&mut Stack` 是唯一允许推进协议状态的访问能力，调用方选择具体的帧提供者（provider）、接口、单调时间和单轮预算。返回值只描述是否仍有协议工作、是否可以立即再运行一轮，以及下一次协议定时期限。

#code-block(
  ```rust
  pub struct PumpOutcome {
      pub work_remaining: bool,
      pub recheck: Recheck,
      pub next_deadline: Option<Instant>,
  }

  impl Stack {
      pub fn pump<P: FrameProvider>(
          &mut self,
          id: InterfaceId,
          provider: &mut P,
          now: Instant,
          budget: PumpBudget,
      ) -> Result<PumpOutcome, PumpError> {
          /* 在有限收发预算内推进真实协议引擎；
             根据设备事件、剩余工作与定时期限返回 PumpOutcome。 */
      }
  }
  ```.text,
  caption: [`Stack::pump` 用泛型帧能力和显式预算推进协议状态（实现节选）],
  lang: "rust",
)

在实际内核运行中，`device/net` 接收网卡设备发布的帧提供者，网络接入模块为它分配一个逻辑网络接口，并把它交给网络工作线程。硬中断只确认设备事件并发出一次重新检查通知；工作线程随后在普通内核线程上下文中独占访问协议栈，完成有预算上限的收发工作，然后释放锁。关机时，网络子系统先关闭新的协议工作入口，再请求工作线程停止，设备驱动继续负责最终关闭中断、设备队列和 DMA 资源。

== 网络域与 IPv4 控制面

网络域指共享一组网络接口、地址与路由策略和协议状态的隔离范围。Anemone 当前使用一个初始网络域，它组合了一份全局协议栈状态、回环接口和已经成功接入的外部逻辑网络接口。我们在这里刻意保留了三种标识：`NetdevId` 标识驱动发布的网络设备，ifindex 和接口名称标识用户可见的逻辑接口，`InterfaceId` 标识协议栈内部的接口映射。它们之间存在受控关联，却不能相互推导或替代。设备重新发布、协议映射回滚和用户可见 ifindex 因而不会意外共享一套生命周期。

IPv4 地址和路由策略由独立的 `Ipv4ControlPlane` 模块维护。它根据启动配置建立 `127.0.0.1/8`、外部地址、直连路由与可选默认路由，并在每次发送前选择源地址和协议接口。协议栈接收选定结果并执行操作，不从 smoltcp 内部路由表反推内核策略。当前静态控制面已经足以支撑 QEMU 与比赛环境中的稳定网络部署，同时也为以后增加动态配置保留了清楚的状态归属。

== 上边界：协议事实如何成为 Linux readiness

每个 TCP 网络端点（Endpoint）的连接阶段、收发容量、对端关闭和待处理错误都由协议栈中的 TCP 模块维护。内核 Socket 适配层只取得一次事实快照，并在自己的边界内计算 Linux poll/epoll 事件。协议栈发出的 Event 只表示“事实可能改变，请重新读取”；它本身不携带 readiness 位图，也不会成为第二份 readiness 状态。

#code-block(
  ```rust
  fn project_public(
      facts: TcpEndpointFacts,
      interests: PollEvent,
  ) -> PollEvent {
      match facts {
          TcpEndpointFacts::Idle | TcpEndpointFacts::Bound => {
              PollEvent::HANG_UP | (PollEvent::WRITABLE & interests)
          }
          TcpEndpointFacts::Listener { has_pending_child } => {
              if has_pending_child && interests.contains(PollEvent::READABLE) {
                  PollEvent::READABLE
              } else {
                  PollEvent::empty()
              }
          }
          TcpEndpointFacts::Connection(connection) => {
              let mut events = PollEvent::empty();
              let pending_error = connection.has_pending_error();

              if pending_error {
                  events |= PollEvent::ERROR;
              }
              if connection.is_terminal() {
                  events |= PollEvent::HANG_UP;
              }
              /* READABLE / WRITABLE / READ_HANG_UP 投影见下段。 */
              events
          }
      }
  }
  ```.text,
  caption: [Socket 层按网络端点形态计算 Linux readiness（节选）],
  lang: "rust",
)

#code-block(
  ```rust
  // TcpEndpointFacts::Connection(connection) 分支（续）
  let mut events = PollEvent::empty();
  let pending_error = connection.has_pending_error();

  if pending_error {
      events |= PollEvent::ERROR;
  }
  if connection.is_terminal() {
      events |= PollEvent::HANG_UP;
  }
  if interests.contains(PollEvent::READABLE)
      && (connection.received_bytes() != 0
          || connection.is_local_read_shutdown()
          || connection.is_peer_receive_closed()
          || pending_error
          || connection.is_terminal())
  {
      events |= PollEvent::READABLE;
  }
  if interests.contains(PollEvent::WRITABLE)
      && (connection.is_local_write_shutdown()
          || pending_error
          || connection.is_terminal()
          || (connection.connect() == TcpConnectFact::Connected
              && connection.send_capacity() != 0))
  {
      events |= PollEvent::WRITABLE;
  }
  if interests.contains(PollEvent::READ_HANG_UP)
      && (connection.is_local_read_shutdown()
          || connection.is_peer_receive_closed()
          || connection.is_terminal())
  {
      events |= PollEvent::READ_HANG_UP;
  }
  events
  ```.text,
  caption: [连接事实继续投影为 `READABLE`、`WRITABLE` 与 `READ_HANG_UP`（节选，续）],
  lang: "rust",
)

这段投影也说明了为什么我们不让后端直接返回 `POLLIN` 或 `POLLOUT`。同一份 TCP 事实需要服务普通 read/write、连接等待、接收连接等待、poll/select 和 epoll；每种操作关心的就绪条件不同。内核保存 Linux 对外承诺，协议栈保存协议真相。等待路径先读取快照，再登记唤醒通知，并在真正休眠前和醒来后重新读取一次，从而避免事实恰好在“检查”和“登记”之间改变而丢失唤醒。

网络端点的生命周期同样遵循这条边界。新 fd 放入进程描述符表之前发生失败时，创建保护对象会回滚尚未发布的网络端点；发布之后，`dup`/`fork` 共享同一个打开文件对象，只有最后一个 fd 引用被移除时才触发一次最终释放。Socket 适配层先禁止新的操作并撤销等待登记，再把非阻塞的 release 命令交给 TCP 模块；FIN、RST、TIME_WAIT 和协议引擎中的最终回收可以继续推进，不会让 `close()` 等待网络握手。这里和Linux的设计一致，从而兼容了一些特殊的语义。

== 后端替换边界

四层架构让协议后端成为一个有明确范围的实现选择。未来采用另一套 TCP/IP 引擎时，我们仍需适配网络接口、网络端点、缓冲区、定时器和协议推进方式；不同协议引擎的能力差异也需要认真处理。稳定边界已经把影响范围收窄：Socket ABI、fd 生命周期、poll/select/epoll、逻辑网络接口、IPv4 路由与选源策略，以及 `device/net` 的帧交接都不依赖 smoltcp 对象。

这项收益对当前代码的价值是：我们可以独立审查协议栈是否泄漏后端对象句柄、Socket 是否复制协议状态、驱动标识是否越过帧接口；任何一类穿透都有清楚的搜索范围和状态属主。后续增加协议或诊断能力时，我们先决定事实属于哪一层，再选择需要暴露的最窄 Command、Event 或 Snapshot。这些设计也许略显繁琐，但是我们换取了清晰的软件工程边界和可控的测试范围。

== 深入真实协议路径的主机测试

Object fence 还提供了一条强大的测试接缝。`FrameProvider` 没有绑定 VirtIO、QEMU 或内核线程，因此主机测试可以实现一个资源数量受限且行为确定的 `BoundedProvider`。测试运行的仍然是正式 `anemone-smoltcp-stack` 和真实 smoltcp 协议引擎，只把设备侧资源、完成顺序与链路事件换成测试能够精确控制的模拟设备。

下面这个测试准备三个待发送数据包，但模拟设备只有两个发送槽。第一次 pump 必须准确停在资源耗尽处；没有完成回收时再次 pump，不能凭空制造进展，也不能进入忙轮询。测试代码回收一个发送槽并发布重新检查通知后，第三个数据包才能继续提交。

#code-block(
  ```rust
  impl FrameProvider for BoundedProvider {
      type RxToken<'a> = DeterministicRxToken<'a>;
      type TxToken<'a> = BoundedTxToken<'a>;

      fn transmit(&mut self, now: Instant)
          -> TransmitOutcome<Self::TxToken<'_>>
      {
          self.observed_at = Some(now);
          if self.facts.link_state != LinkState::Up {
              return TransmitOutcome::LinkUnavailable;
          }
          let Some(index) = self.tx.iter()
              .position(|lane| lane.slot == TxSlot::Available)
          else {
              self.normal_exhaustions += 1;
              return TransmitOutcome::Exhausted;
          };

          self.tx[index].slot = TxSlot::Reserved;
          TransmitOutcome::Ready(BoundedTxToken {
              lane: &mut self.tx[index],
              submission_log: &mut self.submission_log,
              consumed: false,
          })
      }
  }
  ```.text,
  caption: [`BoundedProvider` 用有限 TX slot 建模资源取得与正常耗尽（节选）],
  lang: "rust",
)

#code-block(
  ```rust
  #[test]
  fn deterministic_tx_exhaustion_preserves_and_resumes_socket_work() {
      /* 地址配置与邻居表预热略。 */
      let mut stack = Stack::new();
      let mut provider = BoundedProvider::with_mac(LOCAL_MAC, 2);
      let interface = stack.add_interface(
          &mut provider,
          EthernetAddress::new(LOCAL_MAC),
          Instant::ZERO,
      );

      let packets = [
          build_raw_ipv4_packet(LOCAL_IP, PEER_IP, 1),
          build_raw_ipv4_packet(LOCAL_IP, PEER_IP, 2),
          build_raw_ipv4_packet(LOCAL_IP, PEER_IP, 3),
      ];
      let packet_refs = packets.iter().map(Vec::as_slice).collect::<Vec<_>>();
      stack.queue_ipv4_for_host_validation(interface, &packet_refs).unwrap();

      let exhausted = stack.pump(
          interface,
          &mut provider,
          Instant::from_micros(1),
          PumpBudget::new(1, 3),
      ).unwrap();

      assert_eq!(provider.submissions(), 2);
      assert_eq!(provider.live_tx(), 2);
      assert_eq!(provider.normal_exhaustions(), 1);
      assert!(exhausted.work_remaining);
      assert_eq!(exhausted.recheck, Recheck::Idle);
  }
  ```.text,
  caption: [主机测试构造三个数据包与两个发送槽，并验证首次资源耗尽（节选）],
  lang: "rust",
)

#code-block(
  ```rust
  // 同一测试在资源耗尽后的连续断言（续）
      let still_blocked = stack.pump(
          interface,
          &mut provider,
          Instant::from_micros(2),
          PumpBudget::new(1, 3),
      ).unwrap();
      assert_eq!(provider.submissions(), 2);
      assert!(still_blocked.work_remaining);
      assert_eq!(still_blocked.recheck, Recheck::Idle);

      provider.complete(0);
      provider.publish_recheck();
      assert!(provider.take_recheck());

      stack.pump(
          interface,
          &mut provider,
          Instant::from_micros(3),
          PumpBudget::new(1, 3),
      ).unwrap();
      assert_eq!(provider.submissions(), 3);
      assert_eq!(
          provider.submitted_frames().iter()
              .map(|frame| raw_ipv4_marker(frame))
              .collect::<Vec<_>>(),
          [1, 2, 3]
      );
  ```.text,
  caption: [主机测试验证未完成回收时保持阻塞，并在重新检查通知后恢复推进（节选，续）],
  lang: "rust",
)

同一套模拟设备还覆盖接收帧注入、链路启停、完成回收次序、重复通知合并、未消费 token 的自动归还、超长帧拒绝、回调异常退出和多接口隔离。很多故障在 QEMU 中只能依赖偶然时序触发，在主机测试中可以变成每次都执行相同状态转换的回归测试。我们认为这是四层架构的一个核心优势：我们的测试不仅覆盖局部的单点，还能测试整个状态机闭环。

== 只读网络诊断面

真实网络能力还需要可观察性。常用的 `ip` 工具并不通过 ioctl 读取一张简单配置表，而是使用 `AF_NETLINK` 和 rtnetlink 请求链路、地址与路由的完整列表。我们实现了 Netlink Socket 传输、多段回复、请求序号、`NLMSG_DONE`/`NLMSG_ERROR`、阻塞与非阻塞接收、`MSG_PEEK` 与 `MSG_TRUNC`，再把请求转发给维护相应事实的模块。

诊断适配层在每个请求到来时取得一份独立快照：逻辑接口模块提供 ifindex、名称与接口种类；`device/net` 提供 MAC、MTU 和当前链路状态；`Ipv4ControlPlane` 提供地址、直连路由与默认路由。适配层释放各模块的锁之后，再把快照编码为 Linux 用户接口格式。它不复制一份长期存在的全局网络状态，也不从 smoltcp 私有路由表或对象句柄猜测内核策略。

因此，`ip link show`、`ip address show` 和 `ip route show` 展示的是内核各状态属主在请求时刻提供的只读事实。诊断快照、编码后的 Netlink 数据报和分段读取位置都不能反向影响设备接入、路由选择或数据包处理。这条边界让工具获得真实信息，也保持了单一真相源。

== 真实工作负载闭环

#figure(
  table(
    columns: (4.0cm, 10.2cm),
    align: (center, left),
    inset: 7pt,
    stroke: 0.8pt,
    [*用户操作*], [*贯通的主要系统能力*],
    [`apt install`], [DNS、UDP/TCP Socket、阻塞与 readiness、时间、用户态 TLS、下载数据写入文件系统。],
    [`git clone https://...`], [DNS、TCP 长连接、非阻塞 I/O、用户态加密库、进程与持久化存储。],
    [`ping`], [ICMP raw socket、权限检查、IPv4 路由与源地址选择、外部帧路径与回复接收。],
    [`ip link/address/route show`],
    [AF_NETLINK、rtnetlink 多段回复，以及逻辑接口、网络设备和 IPv4 控制面提供的只读快照。],
  ),
  caption: [真实网络程序覆盖的跨子系统路径],
  kind: table,
)

从这些工作负载回看四层架构，“网络已经连通”只是成果的一部分。Socket、协议状态、协议引擎和设备资源各自拥有清晰的 object fence；同一套语义既能接入真实网卡设备，也能在开发主机上接受细粒度、确定性的压力验证。我们为此付出了比直接嵌入 smoltcp 更多的设计工作，最终——得到了一套更容易解释、验证和继续演进的内核网络栈。
