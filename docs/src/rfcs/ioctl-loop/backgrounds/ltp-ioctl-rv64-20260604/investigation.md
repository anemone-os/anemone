# rv64 LTP ioctl group failure investigation

Date: 2026-06-04
Input log: [`user-test-rv64.log`](./user-test-rv64.log)

This note triages one rv64 user-test run with the active LTP profile group
`ioctl`. The run executes the same 16 case names under both glibc and musl, so
the final `attempted=32 failed=32` summary is 16 distinct failure modes repeated
twice, not 32 independent defects.

## High-level result

The glibc and musl results are identical in shape:

- `ioctl01`: `TBROK unable to open pty: ENOENT`
- `ioctl02`: `TBROK You must specify a tty device with -d option`
- `ioctl03`: `TCONF TUN support is missing?`
- `ioctl04`: loop device found, but `mkfs.ext2` setup breaks with `image is too small`
- `ioctl05`: `BLKGETSIZE` and `BLKGETSIZE64` pass; read at EOF returns `EINVAL`
- `ioctl06`: `BLKRAGET` returns `ENOTTY`
- `ioctl07`: `/dev/urandom` is missing
- `ioctl08`: `btrfs driver not available`
- `ioctl09`: `parted` missing
- `ioctl_loop01..07`: `loop driver not available`

The important split is:

- several cases never reach the tested ioctl because they are blocked by LTP
  prerequisites or rootfs/device visibility;
- a smaller set reaches block/loop ioctl behavior and exposes real semantic
  gaps;
- `ioctl_loop01..07` are currently not measuring loop ioctl semantics at all.

## Runner-level observation

The local group file lists plain case names:

```text
ioctl01
ioctl02
...
ioctl_loop07
```

The upstream LTP runtest entry for `ioctl02` is special: it maps
`ioctl02` to the shell wrapper `test_ioctl`, which scans `/dev/tty*` and invokes
`ioctl02 -d <tty>`. The local parser only supports `case [executable][: args...]`
syntax, so the bare `ioctl02` line runs the C binary directly without `-d`.
That is why it breaks before testing tty `TCGET*` or `TCSET*` semantics.

This is a harness/group-file issue, not yet a tty ioctl result.

## Case classification

| Case | Log symptom | Classification | Likely cause |
| --- | --- | --- | --- |
| `ioctl01` | `openpty()` fails with `ENOENT` | missing tty/pty infrastructure | no usable pty/devpts/ptmx path visible to libc |
| `ioctl02` | missing `-d` option | runner group mismatch | local group should run `test_ioctl` or pass a concrete tty |
| `ioctl03` | `TUN support is missing?` | optional unsupported device class | no `/dev/net/tun` or `/dev/tun` |
| `ioctl04` | `mkfs.ext2: image is too small` | setup failed before BLKRO test | loop-backed device acquisition works, but filesystem formatting path is not usable |
| `ioctl05` | two passes, EOF read returns `EINVAL` | real block-device semantic bug | read at exactly block device EOF should return `0`, not reject the unaligned 1-byte read first |
| `ioctl06` | `BLKRAGET` gets `ENOTTY` | real generic block ioctl gap | block devfs only handles `BLKGETSIZE`, `BLKGETSIZE64`, `BLKSSZGET` |
| `ioctl07` | `/dev/urandom` `ENOENT` | device publish bug plus later ioctl gap | urandom char device is registered but not published into devfs; `RNDGETENTCNT` is also not implemented |
| `ioctl08` | btrfs driver unavailable | accepted infrastructure/feature gap | no btrfs driver/modules metadata |
| `ioctl09` | `parted` missing | rootfs tool gap, with latent loop gaps | even after installing `parted`, this needs `LO_FLAGS_PARTSCAN`, `BLKRRPART`, `/sys/block/loop*` partitions |
| `ioctl_loop01..07` | `loop driver not available` | LTP prerequisite detection false-negative | LTP checks Linux module metadata, not `/dev/loop0` directly |

## Why the loop cases did not test anything

Every `ioctl_loop*` C test declares:

```c
.needs_drivers = (const char *const []) {
    "loop",
    NULL
}
```

LTP checks this through `tst_check_driver("loop")`. That implementation searches
`/lib/modules/<uname.release>/modules.dep` and `modules.builtin`. In this run,
`uname.release` is `6.6.32`, and the log shows:

```text
expected file /lib/modules/6.6.32/modules.dep does not exist or not a file
expected file /lib/modules/6.6.32/modules.builtin does not exist or not a file
loop driver not available
```

This happens before `setup()` in `ioctl_loop01..07`, so none of these tests call
`tst_find_free_loopdev()`, `LOOP_SET_FD`, `LOOP_SET_STATUS`, `LOOP_CHANGE_FD`,
`LOOP_SET_CAPACITY`, `LOOP_SET_DIRECT_IO`, `LOOP_SET_BLOCK_SIZE`,
`LOOP_CONFIGURE`, or `LOOP_GET_STATUS64`.

This is not evidence that `/dev/loop0` is absent. In the same log,
`ioctl04`, `ioctl05`, and `ioctl06` all print:

```text
Found free device '/dev/loop0'
```

The current problem is compatibility between LTP's Linux module-discovery
contract and Anemone's built-in device model.

## Loop implementation gaps hidden behind the prerequisite failure

The current kernel has a loop block device implementation and publishes
`loop0` through block devfs. It supports enough for `tst_find_free_loopdev()`'s
fallback path:

- open `/dev/loop0`
- `LOOP_GET_STATUS` on an unbound device returns `ENXIO`
- `LOOP_SET_FD`
- `LOOP_GET_STATUS` / `LOOP_GET_STATUS64`
- `LOOP_SET_STATUS` / `LOOP_SET_STATUS64`
- `LOOP_CLR_FD`
- generic `BLKGETSIZE`, `BLKGETSIZE64`, `BLKSSZGET`

But the loop LTP cases require more:

- `/dev/loop-control` and `LOOP_CTL_GET_FREE`, or continued reliance on the
  fallback scan
- `/sys/block/loopN/...` attributes such as `size`, `ro`,
  `loop/backing_file`, `loop/autoclear`, `loop/partscan`, `loop/dio`,
  `loop/sizelimit`
- `LO_FLAGS_AUTOCLEAR` and `LO_FLAGS_PARTSCAN` semantics
- partition scanning and loop partition nodes such as `/dev/loop0p1`
- `LOOP_CHANGE_FD`
- `LOOP_SET_CAPACITY`
- `LOOP_SET_DIRECT_IO`
- `LOOP_SET_BLOCK_SIZE`
- `LOOP_CONFIGURE`

Current code explicitly returns `UnsupportedIoctl` for at least
`LOOP_SET_DIRECT_IO` and `LOOP_CONFIGURE`, and does not dispatch
`LOOP_CHANGE_FD`, `LOOP_SET_CAPACITY`, or `LOOP_SET_BLOCK_SIZE`. Therefore, even
if the module metadata check is bypassed, the later loop cases should still
produce real semantic failures.

## Real kernel issues worth prioritizing

1. Make loop-visible tests actually enter their body.

   Minimal options:

   - provide enough `/lib/modules/6.6.32/modules.builtin` metadata for LTP's
     `needs_drivers = "loop"` check; or
   - patch/localize the LTP fixture for Anemone so built-in devices can satisfy
     `tst_check_driver()`.

   This is the immediate reason `ioctl_loop01..07` "did not test".

2. Fix `ioctl05` EOF read semantics.

   `ioctl05` proves that size ioctls and `lseek(end)` already work. The only
   failure is `read(fd, &buf, 1)` at exactly EOF returning `EINVAL`. Linux
   expects `0`. The block-device read path currently validates alignment before
   checking EOF, so an unaligned one-byte read at EOF is rejected instead of
   returning EOF.

3. Add basic generic block ioctls needed by the selected group.

   `ioctl06` needs `BLKRAGET` and `BLKRASET`. `ioctl04` will later need
   `BLKROGET` and `BLKROSET` after its setup gets past `mkfs.ext2`.

4. Publish `/dev/urandom`, then decide the random ioctl boundary.

   The urandom char device is registered, but unlike `null` and `zero`, it is
   not published through devfs. After publishing it, `ioctl07` will then reach
   `RNDGETENTCNT` and `/proc/sys/kernel/random/entropy_avail`, which are not
   covered by the current generic char-device ioctl path.

5. Correct the local `ioctl02` group entry.

   Either use the upstream `test_ioctl` wrapper or encode a valid tty argument.
   Without that, the result is only a runner mistake.

## Accepted or lower-priority infrastructure gaps

- `ioctl03`: TUN/TAP is a network device feature gap.
- `ioctl08`: btrfs and `FIDEDUPERANGE` are out of scope for minimal ioctl/loop
  bring-up.
- `ioctl09`: `parted` is missing from the runtime image. This is a rootfs tool
  gap, but the case would still need partition scan semantics afterward.
- `unknown syscall number 123` appears at the start of many LTP cases. On
  riscv64 Linux this is `sched_getaffinity`. It is noisy in this run, but the
  direct failure points above are later LTP setup or ioctl results, not this
  missing syscall.

## Suggested next diagnostic cut

For the next rv64 run, first make only the loop driver availability check pass
without claiming full loop support. Then rerun only:

```text
ioctl05
ioctl06
ioctl_loop01
ioctl_loop02
ioctl_loop03
ioctl_loop04
ioctl_loop05
ioctl_loop06
ioctl_loop07
```

This should separate:

- already-observed generic block bugs (`ioctl05`, `ioctl06`);
- loop status/flag/sysfs gaps (`ioctl_loop01`, `ioctl_loop07`);
- missing loop private ioctls (`ioctl_loop02..06`);
- rootfs/tool gaps (`parted`) from actual kernel behavior.
