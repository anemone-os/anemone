#import "../components/figure.typ": code-block, report-figure

= Nemophila：基于 WebAssembly 的动态内核扩展

操作系统的许多能力在内核构建时便已确定。新增观测逻辑或扩展功能，通常意味着修改内核源码、重新
编译并重启整套系统。我们希望 Anemone 在保持内核主体清晰可靠的同时，也具备运行期间持续演化的
能力，因此设计并实现了动态内核扩展框架 Nemophila。

Nemophila 让扩展模块既可以作为系统配置的一部分随内核启动，也可以由具备管理权限的用户程序在
系统运行后装载。模块可以接入内核明确提供的观测位置，在完成任务后卸载，并在需要时重新装载。
因此，Anemone 的模块化不再只体现在源码目录和构建选项上，而是延伸到了完整系统的运行过程。

== 受约束且跨架构的模块载体

我们选择 WebAssembly 作为 Nemophila 的模块载体。与直接装载本机机器码相比，WebAssembly 具有
结构化控制流、明确的类型系统和独立线性内存。模块在进入内核运行环境前会经过解析、校验和接口
检查，执行时也不能直接取得任意内核地址或调用任意内核函数，而只能使用 Anemone 明确提供的宿主
接口。这些约束共同构成了一条清楚的能力边界。

模块异常同样由 Nemophila 统一处理。当某个回调执行失败时，我们将故障限制在对应模块实例内，停止
它后续进入内核观测路径，而不会改变被观测任务操作的结果；其它模块和任务子系统仍可继续工作。相比
把扩展直接编译为内核原生代码，这种执行方式既保留了动态性，也让模块的行为更容易检查和管理。

WebAssembly 还把模块制品与处理器指令集解耦。我们可以在 RISC-V64 与 LoongArch64 上加载同一
源码构建、内容完全一致的模块制品，覆盖装载、回调、异常隔离、卸载和重新装载。模块开发者无需为
两种架构分别维护本机二进制，跨架构复用由此成为 Nemophila 的直接能力。

#report-figure(
  image("../assets/nemophila-extension.png", width: 100%),
  caption: [Nemophila 将跨语言 WebAssembly 模块、受约束执行环境与内核观测点连接为完整的动态扩展路径。],
)

== Weave：由内核子系统提供扩展位置

动态扩展并不意味着模块可以任意修改内核。我们为 Nemophila 设计了 Weave 机制：内核子系统在适合
扩展的位置声明观测点，并决定能够向模块提供哪些数据；模块在加载时选择所需观测点并注册回调。
这样，扩展代码获得了接入真实内核路径的能力，而任务、文件系统、网络或设备的核心状态仍由原有
子系统掌握。

这套机制也使观测功能可以按需组合。没有模块接入时，观测点不会改变原有操作；模块接入后，多个
观测点可以在同一个模块中协同工作。Anemone 当前已经在任务创建和线程退出路径提供了 Weave 观测点，
并实现了进程创建观察器和任务关系审计模块。后者能够把一次任务创建与对应的线程退出关联起来，
展示了模块跨越多个内核事件完成实际分析工作的能力。

我们让观测回调只接收完成分析所需的值，而不是向模块暴露任务对象、内部锁或私有数据结构。模块
由此面向稳定、精简的事件接口编程，内核子系统也能够继续独立演进。Weave 不只是一个回调列表，
而是 Anemone 在可扩展性与内核边界之间建立的明确连接方式。

下面的代码取自任务创建和线程退出路径。子任务完成发布并进入调度队列后，内核提交创建者与子任务
的 TID；线程进入不可返回的退出路径、但尚未开始资源清理时，内核提交 TID 与退出原因。两个调用点
都只构造事件值，不把 `Task`、锁或调度器内部状态交给模块。

#code-block(
  ```rust
  // task/api/clone：子任务已经发布并进入调度队列
  enqueue_new_task(published.clone());
  nemophila::CLONE_OBSERVER.notify(
      nemophila::CloneObservation::new(current_task.tid(), new_tid),
  );

  // task/api/exit：已经进入退出路径，资源清理尚未开始
  nemophila::THREAD_EXIT_OBSERVER.notify(
      nemophila::ThreadExitObservation::new(task.tid(), code),
  );
  ```.text,
  caption: [任务创建与线程退出路径中的 Weave 调用点（合并节选）],
  lang: "rust",
)

模块侧则通过 Rust SDK 组合这些观测点。下面是任务关系审计模块的实际 `load` 函数节选：同一份
实例内的数据被两个回调共享，创建事件记录任务关系，退出事件消费这份关系并输出审计结果。模块
只面向事件字段和日志接口编程，不需要依赖内核任务结构。

#code-block(
  ```rust
  fn load(context: &mut LoadContext<'_>) -> Result<(), Self::Error> {
      let audit = Rc::new(RefCell::new(LineageAudit::default()));
      let clone_audit = audit.clone();

      context.weave().task().clone_observer().register(
          move |event, callback| {
              let message = clone_audit.borrow_mut()
                  .observe_clone(event.creator_tid, event.child_tid);
              callback.logging().write(LogLevel::Notice, &message);
          },
      )?;

      context.weave().task().thread_exit().register(
          move |event, callback| {
              let message = audit.borrow_mut().observe_exit(event);
              callback.logging().write(LogLevel::Notice, &message);
          },
      )?;
      Ok(())
  }
  ```.text,
  caption: [`task-lineage-auditor` 使用 Rust SDK 组合两个观测点（节选）],
  lang: "rust",
)

== 从构建到卸载的完整生命周期

Nemophila 与 Anemone 的构建系统共同管理模块制品。系统配置可以选择启动时必需的模块，构建系统会
从模块清单生成新制品、完成检查，并把当前构建对应的内容固定到内核输入中。运行期间，管理程序也
可以从普通文件装载模块。这两种来源最终进入同一套校验、实例创建和发布流程，因此模块无论在何时
进入系统，都具有一致的运行方式。

一次典型的动态扩展过程包括以下步骤：

- 模块开发者使用 SDK 和普通语言工具链生成 WebAssembly 制品；
- Nemophila 在装载时检查制品与接口，并让模块登记所需的 Weave 观测点；
- 全部准备完成后，模块实例和观测关系一起对内核生效；
- 内核事件触发回调，模块通过受约束接口记录或处理信息；
- 管理程序可以查看实例状态（我们接入了procfs），并在回调结束后卸载模块；
- 卸载会同时撤销观测关系并释放实例，之后同一制品可以作为新实例再次装载。

我们把“能执行一段 WebAssembly”推进成了从制品构建、授权装载、真实回调、状态观察、故障隔离到
安全卸载的完整纵向闭环。这个闭环是 Nemophila 成为内核扩展框架，而不仅是解释器演示的关键。

== 面向多语言的模块生态

Nemophila 使用 WIT 描述模块与内核之间的语言无关接口。开发语言只要能够生成符合接口约定的
WebAssembly 制品，就可以沿同一条路径接入 Anemone。项目已经提供 `no_std` Rust SDK，并用它实现和
验证了首批真实模块；该接口形式也为 Zig、AssemblyScript 等支持 WebAssembly 的语言提供了接入
基础。

SDK 把模块入口、日志和 Weave 回调包装成面向模块作者的直接接口。开发者可以专注于扩展逻辑，模块
如何注册、如何调用宿主能力以及如何生成可交付制品，则由 SDK 和构建系统共同处理。我们希望借助
成熟的 WebAssembly 语言与工具生态，降低内核扩展的开发门槛，同时维持统一的安全边界和生命周期。

== 与 eBPF 的关系

Nemophila 与 eBPF 都赋予内核运行期可编程能力，也在观测和扩展场景中存在交集。我们的目标并不是
在 Anemone 中复刻或替代 Linux eBPF，而是选择另一种抽象中心：以标准 WebAssembly 作为跨架构模块
制品，以语言无关接口连接开发工具链，以 Weave 表达由内核子系统提供的扩展位置，再用统一运行环境
管理完整模块生命周期。

eBPF 已经形成成熟的 Linux 生态；Nemophila 则让我们能够围绕 Anemone 自身的模块边界和双架构能力
探索一条不同的路径。它把安全执行、跨语言开发、动态观测和系统构建组合在一起，成为 Anemone 从
功能完整的宏内核走向可动态扩展内核平台的重要一步。
