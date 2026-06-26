= ABI 兼容设计

== 兼容范围

系统调用层承担 Linux ABI translation：syscall number、参数解析、UAPI struct、flag、errno 和返回值都在这一层收束。正式报告不需要列完整 syscall 表，但需要选择若干代表路径，例如 `openat`、`clone`、`mmap`、`ioctl`、`pselect6`，说明 Anemone 如何把用户可见协议转换为内部 typed API。

== 系统调用分发

本节说明 syscall dispatch、参数解析和 syscall 实现注册方式。正文应把分发路径和具体模块连接起来，而不是只列 syscall 表。

== 参数、flag 与 errno

Linux 兼容性的风险经常出现在 flag 解析、结构体布局、错误码和边界条件上。本节需要选择若干代表 syscall 说明 Anemone 如何处理 UAPI 输入，并把内部错误映射回用户可见 errno。

== 代表路径

本节建议展开 `openat`、`clone`、`mmap`、`ioctl`、`pselect6` 等路径。写法应聚焦“用户可见协议如何落到前文介绍过的 task、VFS、MM、device、wait 等模块”，避免把 ABI 写成抽象设计宣言。

== 阶段性兼容策略

对于阶段性不支持或静默兼容的 flag，需要说明可见行为、日志、退出条件和当前限制。开发报告应避免把兼容缺口包装成完整支持；评审更关心团队是否能解释边界、风险和后续路线。
