= ABI 兼容设计

== 兼容范围

系统调用层承担 Linux ABI translation：syscall number、参数解析、UAPI struct、flag、errno 和返回值都在这一层收束。报告不列完整 syscall 表，而是通过 `openat`、`clone`、`mmap`、`ioctl`、`pselect6` 等代表路径说明 Anemone 如何把用户可见协议转换为内部 typed API。

== 系统调用分发

syscall dispatch、参数解析和 syscall 实现注册方式把用户态入口连接到具体内核模块。这里关注分发路径如何落到 task、VFS、MM、device 和 wait 等机制，而不是罗列 syscall 表。

== 参数、flag 与 errno

Linux 兼容性的风险经常出现在 flag 解析、结构体布局、错误码和边界条件上。Anemone 在 syscall 边界处理 UAPI 输入，并把内部错误映射回用户可见 errno。

== 代表路径

`openat`、`clone`、`mmap`、`ioctl`、`pselect6` 等路径连接了用户可见协议和前文介绍过的 task、VFS、MM、device、wait 等模块，能够代表 Anemone 的 ABI 兼容策略。

== 阶段性兼容策略

对于阶段性不支持或静默兼容的 flag，需要说明可见行为、日志、退出条件和当前限制。开发报告应避免把兼容缺口包装成完整支持；评审更关心团队是否能解释边界、风险和后续路线。
