# Epoll 当前契约

**Owner：** 每个 `Epoll` instance 及其 `EpollWatch` / operation / wait-publication protocol
**覆盖范围：** watch identity与policy、bounded readiness scan、ET/ONESHOT、copyout commit、epoll-file pollability与首版nested rejection
**不覆盖：** 具体target readiness predicate、source route registry、opened-description refcount、scheduler wait completion、socket readiness
**最后核验：** 2026-08-02

本目录登记已经由 `EPOLL-CUTOVER` 生效的共享规则。具体source仍唯一拥有其
readiness truth与route storage；epoll只拥有watch、delivery policy与instance-local操作协议。

## Contract Surfaces

- [Epoll protocol](./protocol.md)：watch/lifecycle、operation-serialized bounded scan与non-sleeping epoll-file publication。

## 邻接契约

- [Poll wait 与source registration](../iomux/poll-wait.md)：`PollRoute`、`SubscribedRecheck`与final snapshot gate。
- [Opened-description lifecycle](../task/opened-description-lifecycle.md)：non-owning identity/liveness capability与operation-local lease。
- [Scheduler Latch wait round](../scheduler/latch-wait-round.md)：单轮task wait identity与completion。
- [Temporary-mask delivery](../signal/temporary-mask-delivery.md)：`epoll_pwait*` mask、reservation与restore owner。
- [Unix Socket state与stream](../socket/unix-stream-lifecycle.md)：direction owner提供独立RDHUP/HUP current predicate。
