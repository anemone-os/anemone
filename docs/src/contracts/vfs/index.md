# VFS 当前契约

**Owner：** VFS inode kind、filesystem-type identity、legacy mount admission protocol 与 local whole-file flock protocol
**覆盖范围：** 本轮按触达提取的 inode file-kind truth、Linux mode projection、canonical filesystem identity、mount source-kind admission、syscall-only fstype alias containment与inode-associated local flock
**不覆盖：** mount topology / namespace、mount attrs、unmount lifecycle、filesystem discovery、filesystem-private mount data、remote flock、record locks
**最后核验：** 2026-07-31

本目录只登记已经由 live code 与验证完成 cutover 的 VFS 共享规则，不声称枚举 VFS 全部不变量。

## Contract Surfaces

- [File kind 与 Linux mode projection](./file-kind.md)：immutable inode kind、ordinary anonymous control object 与 `S_IFMT` projection。
- [Mount admission](./mount-admission.md)：filesystem identity、no-device / block-device source requirement 与 legacy syscall admission。
- [Local whole-file flock](./flock.md)：inode-associated grant truth、blocking recheck与terminal opened-description cleanup。

## 邻接契约

- [Procfs 当前契约](../procfs/index.md)：procfs 的其它只读 ABI projection。
- [Opened-description lifecycle](../task/opened-description-lifecycle.md)：flock holder identity、terminal liveness与mandatory retirement handoff。
- [Asynchronous wake delivery](../scheduler/wake-delivery.md)：notification之后的logical completion与physical placement ownership。
