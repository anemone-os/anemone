# VFS 当前契约

**Owner：** VFS filesystem-type identity 与 legacy mount admission protocol
**覆盖范围：** 本轮按触达提取的 canonical filesystem identity、mount source-kind admission 与 syscall-only fstype alias containment
**不覆盖：** mount topology / namespace、mount attrs、unmount lifecycle、filesystem discovery、filesystem-private mount data
**最后核验：** 2026-07-24

本目录只登记已经由 live code 与验证完成 cutover 的 VFS 共享规则，不声称枚举 VFS 全部不变量。

## Contract Surfaces

- [Mount admission](./mount-admission.md)：filesystem identity、no-device / block-device source requirement 与 legacy syscall admission。

## 邻接契约

- [Procfs 当前契约](../procfs/index.md)：procfs 的其它只读 ABI projection。

