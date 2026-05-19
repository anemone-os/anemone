# Anemone Kernel Signal 系统详解

## 一、概述

Anemone 实现了 POSIX.1-2001 标准的信号机制，API 层完全兼容 Linux 用户空间 (uapi)。内核设计参考了 Linux 的信号实现，采用相同的语义和数据结构编码，但具体实现架构是独立设计的。

信号号范围为 **1-63**，其中 1-31 为不可靠信号 (unreliable signals)，32-63 为实时信号 (realtime signals, POSIX.1b)。

---

## 二、核心数据结构

### 2.1 SigNo — 信号编号

**文件:** `anemone-kernel/src/task/sig/mod.rs:43-101`

```rust
pub struct SigNo(usize);
```

- 封装信号编号 (1–63)，0 为非法值
- 提供 `is_realtime()` / `is_unreliable()` 判断
- 提供 `realtime_index()` 获取实时信号在队列数组中的索引
- SIGKILL 和 SIGSTOP 具有特殊地位：不可被阻塞 (mask)、不可被忽略、不可自定义处理函数

### 2.2 Signal — 待发送/已发送的信号

**文件:** `anemone-kernel/src/task/sig/mod.rs:103-164`

```rust
pub struct Signal {
    no: SigNo,           // 信号编号
    errno: i32,          // si_errno
    code: SiCode,        // 信号来源
    fields: SigInfoFields, // 信号附加信息
}
```

### 2.3 SigSet — 信号集（位图）

**文件:** `anemone-kernel/src/task/sig/set.rs`

```rust
pub struct SigSet(u64);
```

- 64 位位图，bit 0 对应信号 1，bit 62 对应信号 63，bit 63 未使用
- 支持集合运算：并 (union)、交 (intersection)、差 (difference)、补 (complement)、子集判断 (contains)
- 用于 signal mask 和 pending signal set 的表示

### 2.4 SiCode — 信号来源码

**文件:** `anemone-kernel/src/task/sig/info.rs:8-52`

```rust
pub enum SiCode { Kernel, User, Queue, TKill }
```

- `Kernel` (SI_KERNEL = 0x80): 内核产生 (缺页异常、非法指令等)
- `User` (SI_USER = 0): 用户通过 kill() 发送
- `Queue` (SI_QUEUE = -1): 通过 sigqueue() 发送
- `TKill` (SI_TKILL = -6): 通过 tgkill() 发送
- 正值表示内核产生 (`from_kernel()`)，负值或零表示用户产生 (`from_user()`)

### 2.5 SigInfoFields — 信号附加信息

**文件:** `anemone-kernel/src/task/sig/info.rs:54-174`

```rust
pub enum SigInfoFields {
    Kill(SigKill),   // kill/tgkill 发送的信号
    Rt(SigRt),       // 实时信号 (sigqueue)
    Chld(SigChld),   // SIGCHLD (子进程状态变更)
    Fault(SigFault), // 硬件异常 (SIGSEGV, SIGBUS 等)
    TKill(SigKill),  // tgkill 专用
    Ill(SigFault),   // 非法指令 (SIGILL)
}
```

各种 fields 子结构：
- `SigKill`: 发送者 pid + uid
- `SigRt`: 发送者 pid + uid + sigval (附带数据)
- `SigChld`: 子进程 pid + uid + 退出状态 + CPU 时间
- `SigFault`: 故障地址

### 2.6 PendingSignals — 未决信号队列

**文件:** `anemone-kernel/src/task/sig/mod.rs:166-351`

```rust
pub struct PendingSignals {
    unreliable: [Option<Signal>; NUNRELIABLESIG + 1], // 31个槽位
    realtime: [VecDeque<Signal>; NRTSIG],              // 32个队列
}
```

**核心规则：**
- **不可靠信号 (1-31)**: 每个信号号只有一个槽位，后来的信号覆盖旧的，可能丢失
- **实时信号 (32-63)**: 每个信号号有一个 FIFO 队列，信号按到达顺序排队，不会丢失

**取出信号的方法：**
- `fetch_any(mask)`: 按优先级取一个未被 mask 的信号
  1. 先取 SIGKILL（最高优先）
  2. 再取 SIGSTOP
  3. 然后按信号号从小到大扫描实时信号队列
  4. 最后扫描不可靠信号
- `fetch_specific(set)`: 只取 set 中指定的信号
- 被忽略的信号根本不会被放入 PendingSignals（在 `recv_signal` 阶段就被过滤掉了）

### 2.7 SignalAction / KSigAction / SignalDisposition — 信号处置

**文件:** `anemone-kernel/src/task/sig/disposition.rs`

```rust
pub enum SignalAction {
    Default(fn(SigNo)),  // 默认动作
    Ignore,              // 忽略
    Custom(VirtAddr),    // 用户自定义处理函数地址
}

pub struct KSigAction {
    pub action: SignalAction,
    pub flags: SaFlags,
    pub mask: SigSet,    // 执行处理函数时临时阻塞的信号集
}

pub struct SignalDisposition {
    actions: [SignalAction; NSIG], // 64 个
    flags: [SaFlags; NSIG],
    masks: [SigSet; NSIG],
}
```

- 采用 **Struct-of-Arrays** 而非 Array-of-Structs 布局，对缓存和 SIMD 更友好
- 每个信号有独立的三元组 (action, flags, mask)
- SIGKILL 和 SIGSTOP 的 disposition 不可更改

**SaFlags 标志位：**
| 标志 | 含义 |
|------|------|
| `SIGINFO` (SA_SIGINFO) | 使用三参数形式的信号处理函数 |
| `ONESHOT` (SA_ONESHOT) | 处理一次后自动恢复为默认动作 |
| `NODEFER` (SA_NODEFER) | 处理期间不自动阻塞当前信号 |
| `ONSTACK` (SA_ONSTACK) | 在备选信号栈上执行处理函数 |
| `RESTART` (SA_RESTART) | 自动重启被信号中断的可重启系统调用 |
| `RESTORER` | 兼容标志 (musl 可能设置，但内核不使用) |

**默认动作分类：**
| 动作 | 信号 |
|------|------|
| `terminate` (终止进程) | SIGHUP, SIGINT, SIGKILL, SIGTERM, SIGALRM, SIGUSR1/2, SIGPIPE, SIGSTKFLT, SIGVTALRM, SIGPROF, SIGIO, SIGPWR, 所有实时信号 |
| `core_dump` (终止+核心转储) | SIGQUIT, SIGILL, SIGTRAP, SIGABRT, SIGFPE, SIGSEGV, SIGBUS, SIGXCPU, SIGXFSZ, SIGSYS |
| `ignore` (忽略) | SIGCHLD, SIGURG, SIGWINCH |
| `stop` (暂停) | SIGSTOP, SIGTSTP, SIGTTIN, SIGTTOU |
| `cont` (继续) | SIGCONT |

### 2.8 SigAltStack — 备选信号栈

**文件:** `anemone-kernel/src/task/sig/altstack.rs`

```rust
pub struct SigAltStack {
    stack_base: VirtAddr,
    stack_bytes: usize,
    flags: SigAltStackFlags,
}
```

纯记账结构，内存管理由用户空间负责。

### 2.9 RtSigFrame — 信号栈帧

**文件:** `anemone-kernel/src/task/sig/hal.rs:46-93`

```rust
#[repr(C)]
pub struct RtSigFrame {
    pub siginfo: SigInfoWrapper,   // 信号信息 (第二个参数)
    pub ucontext: UContext,        // 用户上下文 (第三个参数)
}
```

该结构体在执行用户信号处理函数之前，被压入用户栈（或备选信号栈），包含：
- `siginfo`: 传递给信号处理函数的第二个参数 (可通过 SA_SIGINFO 启用)
- `ucontext`: 包含保存的寄存器状态、信号掩码、信号栈信息

---

## 三、参与系统

### 3.1 每个 Task (任务控制块) 中的信号字段

**文件:** `anemone-kernel/src/task/mod.rs:128-135`

```
sig_disposition  Arc<RwLock<SignalDisposition>>  信号处置表 (线程组内共享)
sig_mask         SpinLock<SigSet>                 阻塞信号掩码 (每线程独立)
sig_pending      SpinLock<PendingSignals>         私有未决信号 (每线程独立)
sig_altstack     SpinLock<Option<SigAltStack>>    备选信号栈 (每线程独立)
```

### 3.2 每个 ThreadGroup (线程组) 中的信号字段

**文件:** `anemone-kernel/src/task/mod.rs:283-284`

```
sig_pending       SpinLock<PendingSignals>   线程组共享未决信号
terminate_signal  Option<SigNo>              线程组退出时发给父进程的信号
```

### 3.3 调度器 — notify()

**文件:** `anemone-kernel/src/sched/mod.rs:190-214`

`notify()` 函数用于唤醒任务：
- 如果任务处于 `Waiting { interruptible: true }` → 唤醒
- 如果任务处于 `Waiting { interruptible: false }` 且 uninterruptible 参数为 true → 唤醒
- 如果已经是 Runnable 或 Zombie → 无操作
- 唤醒后将任务重新加入运行队列 (`task_enqueue`)

### 3.4 事件系统 (Event/Listener) — 可中断等待

**文件:** `anemone-kernel/src/sched/event.rs`

`Event::listen()` 在每个循环迭代中检查 `task.has_unmasked_signal()`，如果有未屏蔽的信号则返回 false 表示被信号中断，从而让调用者处理信号。

### 3.5 架构抽象层 — SignalArch

**文件:** `anemone-kernel/src/task/sig/hal.rs:11-44`

```rust
pub trait SignalArchTrait {
    const MINSIGSTKSZ: usize;

    fn encode_ucontext(buf: &mut UContext, trapframe: &TrapFrame,
                       mask: SigSet, altstack: SigStack);
    fn restore_ucontext(ucontext: &UContext, trapframe: &mut TrapFrame);
    fn prepare_trapframe_for_signal_handler(
        trapframe: &mut TrapFrame,
        signo: SigNo, handler: VirtAddr, sigframe_base: VirtAddr,
    );
}
```

**RISC-V 64 实现:** `anemone-kernel/src/arch/riscv64/exception/trap/signal.rs`
**LoongArch 64 实现:** `anemone-kernel/src/arch/loongarch64/exception/trap/signal.rs`

### 3.6 Trap (陷入) 处理路径

**RISC-V 64:** `anemone-kernel/src/arch/riscv64/exception/trap/utrap.rs`
**LoongArch 64:** `anemone-kernel/src/arch/loongarch64/exception/trap/utrap.rs`

陷入处理是信号投递的关键时机。在 `rust_utrap_entry` 函数中，处理完异常/中断后，总是在返回用户空间之前调用 `handle_signals()`。

### 3.7 内存管理 — 缺页异常

**文件:** `anemone-kernel/src/mm/uspace/fault.rs`

当用户空间访问非法地址导致缺页异常无法解决时，发送 SIGSEGV 信号。

### 3.8 进程退出 (Exit)

**文件:** `anemone-kernel/src/task/api/exit/mod.rs`

- `kernel_exit_group(ExitCode::Signaled(sig))`: 终止线程组
- 线程组退出时，如果配置了 `terminate_signal`（如 SIGCHLD），则会发送信号给父线程组
- 退出前收集 CPU 使用时间作为 SIGCHLD 的 utime/stime
- `kernel_exit_group` 会向线程组内所有其他成员发送 SIGKILL
- `dethread` 操作也会向线程组内其他成员发送 SIGKILL

### 3.9 Clone (进程/线程创建)

**文件:** `anemone-kernel/src/task/api/clone/mod.rs`

- `CLONE_SIGHAND`: 子任务共享父任务的 `SignalDisposition`（信号处置表）
- `CLONE_THREAD`: 必须与 CLONE_SIGHAND 一起使用，子任务加入同一线程组
- 线程组内的所有线程共享同一份 `SignalDisposition`，共享线程组级别的 `sig_pending`
- 每个线程有自己独立的 `sig_mask`、私有 `sig_pending`、独立的 `sig_altstack`

### 3.10 系统调用接口

所有信号相关的系统调用位于 `anemone-kernel/src/task/sig/api/`：

| 系统调用 | 文件 | 功能 |
|---------|------|------|
| `kill` | `kill.rs` | 向线程组/进程组/广播发送信号 |
| `tgkill` | `tgkill.rs` | 向指定线程发送信号 |
| `rt_sigaction` | `rt_sigaction.rs` | 设置/获取信号处置 |
| `rt_sigprocmask` | `rt_sigprocmask.rs` | 阻塞/解除阻塞/设置信号掩码 |
| `rt_sigpending` | `rt_sigpending.rs` | 查询当前未决信号集 |
| `rt_sigreturn` | `rt_sigreturn.rs` | 从信号处理函数返回，恢复上下文 |
| `rt_sigtimedwait` | `rt_sigtimedwait.rs` | 同步等待信号（带超时） |
| `rt_sigqueueinfo` | `rt_sigqueueinfo.rs` | 向线程组发送带附加数据的信号 |
| `sigaltstack` | `sigaltstack.rs` | 设置/获取备选信号栈 |
| `ppoll` | `../fs/api/iomux/ppoll.rs` | 涉及信号掩码原子的 poll |

---

## 四、完整运作流程

### 阶段 A: 信号发送

```
用户空间: kill(pid, SIGTERM) 或 tgkill(tgid, tid, SIGUSR1)
            │
            ▼
    ┌───────────────────┐
    │ 系统调用入口       │  (kill.rs / tgkill.rs / rt_sigqueueinfo.rs)
    │ 构造 Signal 对象   │
    └───────┬───────────┘
            │
            ▼
    ┌───────────────────────────────────────┐
    │ Task::recv_signal()    (per-thread)   │
    │ 或                                    │
    │ ThreadGroup::recv_signal()             │
    │                                        │
    │ 1. 读取 SignalDisposition             │
    │ 2. 如果 action.is_ignored() → 丢弃    │
    │ 3. 调用 sig_pending.push_signal()     │
    │    - 不可靠信号: 覆盖旧值              │
    │    - 实时信号: push_back 到队列        │
    │ 4. 检查 sig_mask                      │
    │    - 如果被 mask 且非 SIGKILL/STOP:    │
    │      不做通知，等任务自行 unmask       │
    │    - 否则: notify(task) → 唤醒任务    │
    └───────────────────────────────────────┘
```

**特殊情况：**
- **内核生成的信号** (缺页 → SIGSEGV，非法指令 → SIGILL)：直接在 trap handler 中调用 `task.recv_signal()` 或 `tg.recv_signal()`
- **exit_group**: 向线程组其他成员发送 SIGKILL
- **dethread**: 向线程组其他成员发送 SIGKILL

---

### 阶段 B: 信号投递（返回用户空间时）

```
用户任务陷入内核 (syscall / exception / interrupt)
            │
            ▼
    ┌──────────────────────────┐
    │ rust_utrap_entry()       │  (arch 特定的陷入入口)
    │ - 保存 trapframe         │
    │ - 处理异常/系统调用/中断  │
    └──────────┬───────────────┘
               │
               ▼
    ┌──────────────────────────┐
    │ handle_signals()         │  (task/sig/mod.rs:602)
    │                          │
    │ loop {                   │
    │   signal = task          │
    │     .fetch_signal()      │
    │   if signal is None:     │
    │     break                │
    │   perform_signal_action()│
    │ }                        │
    └──────────┬───────────────┘
               │
               ▼
    ┌──────────────────────────────────────┐
    │ task.fetch_signal()                  │
    │                                      │
    │ 1. 从私有 sig_pending 取信号         │
    │    (使用 fetch_any, 跳过被 mask 的)  │
    │ 2. 如果没有 → 从线程组共享           │
    │    sig_pending 取信号                │
    │ 3. 优先级:                           │
    │    SIGKILL > SIGSTOP > 实时信号       │
    │    (按号排序) > 其他不可靠信号        │
    └──────────┬───────────────────────────┘
               │
               ▼
    ┌────────────────────────────────────┐
    │ perform_signal_action()            │
    │                                    │
    │ 根据 SignalAction 分派:            │
    └──┬─────────┬──────────┬────────────┘
       │         │          │
       ▼         ▼          ▼
    Default   Ignore     Custom(handler_addr)
       │         │          │
       │         │          ├─ 1. 更新 sig_mask
       │         │          │     mask |= handler_mask
       │         │          │     if !NODEFER: mask |= {当前信号}
       │         │          │
       │         │          ├─ 2. 确定栈位置
       │         │          │     检查 SA_ONSTACK + altstack
       │         │          │     已在altstack上则继续使用
       │         │          │     否则使用新栈或当前栈
       │         │          │
       │         │          ├─ 3. SA_RESTART处理
       │         │          │     恢复可幂等重启的系统调用上下文
       │         │          │
       │         │          ├─ 4. 构造 UContext
       │         │          │     SignalArch::encode_ucontext()
       │         │          │     保存: PC, GPR, sigmask, altstack
       │         │          │
       │         │          ├─ 5. 构造 RtSigFrame
       │         │          │     { siginfo, ucontext }
       │         │          │     压入用户栈/altstack
       │         │          │
       │         │          ├─ 6. 设置 trapframe
       │         │          │     SignalArch::
       │         │          │     prepare_trapframe_for_signal_handler()
       │         │          │     - PC → handler 地址
       │         │          │     - RA → __sys_rt_sigreturn (trampoline)
       │         │          │     - SP → sigframe_base
       │         │          │     - a0 → signo
       │         │          │     - a1 → &siginfo
       │         │          │     - a2 → &ucontext
       │         │          │
       │         │          └─ 7. break_loop = true
       │         │               (不再处理更多信号)
       │         │
       ▼         ▼
    ┌─────────────────────────┐
    │ 执行默认动作:             │
    │ terminate → kernel_exit  │
    │            _group()      │
    │ core_dump → kernel_exit  │
    │            _group()      │
    │ ignore    → 无操作        │
    │ stop      → (NYI)        │
    │ cont      → (NYI)        │
    └─────────────────────────┘

    处理完所有信号后，如果设置了 break_loop，则不再继续循环
            │
            ▼
    ┌──────────────────────────┐
    │ 返回用户空间              │  (arch 特定的 trap 返回路径)
    │ sret / ertn              │
    └──────────────────────────┘
```

---

### 阶段 C: 信号处理函数执行与返回

```
用户空间信号处理函数执行:
    void handler(int signo, siginfo_t *info, void *ucontext)
    处理完毕后 return
            │
            ▼
    返回到 RA 寄存器指向的地址
    = __sys_rt_sigreturn (trampoline)
            │
            ▼
    ┌──────────────────────────────┐
    │ __sys_rt_sigreturn:          │  (arch 特定的汇编 trampoline)
    │   li a7, SYS_RT_SIGRETURN    │
    │   ecall / syscall 0          │
    └──────────┬───────────────────┘
               │
               ▼
    ┌──────────────────────────────┐
    │ sys_rt_sigreturn()           │  (task/sig/api/rt_sigreturn.rs)
    │                              │
    │ 1. 从用户栈读取 RtSigFrame  │
    │ 2. validate() 安全检查       │
    │    - 检查栈地址合法           │
    │    - 检查 PC 为用户空间地址   │
    │ 3. SignalArch::             │
    │    restore_ucontext()        │
    │    恢复 PC, GPR 到 trapframe │
    │ 4. 恢复 sig_mask             │
    │    - 检查 bit 63 未设置      │
    │    - 检查 SIGKILL/STOP       │
    │      未被 block              │
    └──────────┬───────────────────┘
               │
               ▼
    返回到中断前的用户代码继续执行
    (trapframe.sepc 已被恢复为原始值)
```

---

### 阶段 D: 同步等待信号 — sigtimedwait

```
用户空间: sigtimedwait(&set, &info, timeout)
            │
            ▼
    ┌────────────────────────────────────┐
    │ sys_rt_sigtimedwait()              │
    │                                    │
    │ 1. 读取等待的信号集 (uthese)       │
    │ 2. 保存当前 sig_mask               │
    │ 3. 从当前 mask 中移除 uthese        │
    │    (临时解除阻塞)                   │
    │ 4. 检查是否有匹配的 pending 信号   │
    │    - 有 → 取出信号，返回           │
    │    - 无 → schedule_with_timeout()  │
    │      (进入可中断睡眠)              │
    │ 5. 被唤醒后再次检查 pending 信号   │
    │ 6. 恢复原始 sig_mask               │
    │ 7. 未等到 → EAGAIN                 │
    │    被其他信号中断 → EINTR           │
    │    等到信号 → 返回信号号            │
    └────────────────────────────────────┘
```

---

## 五、特殊信号处理

### 5.1 SIGKILL (不可屏蔽，不可忽略)

- 在任何信号发送/投递/排查路径中都有特殊处理
- 即使被 mask 也会通知任务
- 不可通过 sigaction 更改处置
- 默认动作：terminate

### 5.2 SIGSTOP (不可屏蔽，不可忽略)

- 与 SIGKILL 一样具有特殊地位
- 默认动作：stop (暂停进程执行)
- 后续可被 SIGCONT 恢复

### 5.3 SIGCHLD (子进程状态变更)

- 在 `kernel_exit` 中，线程组退出时根据 `terminate_signal` 决定发送哪种信号
- 父进程通常配置 SIGCHLD，但也可以配置其他信号
- 附带子进程的 pid、退出状态、CPU 使用时间

### 5.4 SIGPIPE (向已关闭的管道写入)

- 目前仅定义了默认动作为 terminate，尚未在管道代码中实际触发

---

## 六、锁顺序

根据 `Task` 结构体的文档（`task/mod.rs:59-61`）:

```
uspace → flags → name
sig_pending → sig_mask → sig_disposition
```

完整的锁链包含 TOPOLOGY 锁（全局任务拓扑读写锁）：

```
TOPOLOGY → ThreadGroup.inner → sig_pending → sig_mask → sig_disposition
```

---

## 七、关键设计决策

1. **Struct-of-Arrays 布局**: `SignalDisposition` 使用三个独立数组而非结构体数组，对缓存和 SIMD 更友好。

2. **忽略信号的早期过滤**: 被忽略的信号不会进入 `PendingSignals`，在 `recv_signal` 阶段就被丢弃，简化后续流程。

3. **私有+共享 pending**: 每个线程有自己的 `sig_pending`，线程组有共享的 `sig_pending`。信号投递时先检查私有再检查共享。

4. **trampoline 机制**: 使用内核提供的 `__sys_rt_sigreturn` 作为信号处理函数的返回地址，无需用户空间提供 restorer 函数。

5. **信号栈重入处理**: 如果信号处理函数执行期间再次触发信号，且已经处于 altstack 上，则继续在 altstack 上压入新帧（而非切换到新栈）。

6. **系统调用重启**: 当 `SA_RESTART` 设置时，只有可幂等的系统调用会被重启。`restart_syscall` 使用 `take()` 确保只有第一个安装 SA_RESTART 的信号处理函数可以重启系统调用。

7. **可中断等待**: `Event::listen()` 在每次循环迭代中检查 `has_unmasked_signal()`，确保信号能及时中断阻塞等待。
