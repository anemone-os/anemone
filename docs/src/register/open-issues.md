# 开放问题

## ANE-20260527-LTP-CHDIR01-DEVICE-POOL

**Type:** Issue
**Status:** Open
**Area:** user-test / LTP / device model

**Symptom / Trigger:** 在 rv64 白名单跑到 `chdir01` 时，`tst_device` 可能拿不到可用设备，随后测试以 `TBROK: Failed to acquire device` 结束。

**Impact:** 会把一次本应聚焦内核语义的白名单验证变成环境失败，遮蔽后续回归判断。

**Owner:** doruche
**Last Verified:** 2026-05-27
**Exit Condition:** 白名单运行时稳定提供足够的可用设备，或者 `chdir01` 不再依赖当前这套设备池约束。
**Related:** [开发日志：2026-05-25 至 2026-06-07](../devlog/2026-05-25_to_2026-06-07.md)

**Severity:** Low
**Workaround:** 重新整理设备占用后再跑，或在专门的验证环境中执行该用例。
