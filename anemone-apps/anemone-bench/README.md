# Anemone Bench

Anemone Bench vendors and adapts libc-bench from the musl project:

- origin: https://git.musl-libc.org/cgit/libc-bench/
- commit: b6b2ce5f9f87a09b14499cb00c600c601f022634
- license: MIT, reproduced in COPYRIGHT

The local harness adds case selection, repetition, an architecture-neutral
pthread creation count, explicit CPU-only baselines, monotonic timing, strict
child/error handling, and one-line result records. The imported benchmark
payloads otherwise retain the upstream workload shapes.

The source tree keeps CLI and case composition in `src/main.c`, process and
measurement support in `src/harness/`, and imported workload families in
`src/suites/`. Interactive runs add ANSI-colored session and case headings when
stdout is a terminal. `ANEMONE_BENCH` and `BENCH` records remain plain text for
result collection, and redirected output contains no ANSI escapes.

Build and export a static binary for the current Linux host, then list its
cases with:

    just app build --arch host anemone-bench
    ./build/apps/anemone-bench/anemone-bench --list

Set `HOST_CC` to select a different host compiler. Xtask selects the declared
host target and exports the artifact, while the app-owned command remains
responsible for compiler and standard-library availability.

Use --list to discover case names. A focused pthread run looks like:

    anemone-bench --case pthread.createjoin_serial1 --pthread-count 2500 --repeat 5

A syscall-free CPU baseline can be scaled independently:

    anemone-bench --case cpu.integer --cpu-iterations 50000000

Focused virtual-memory cases keep the operation count and page-table placement
explicit. This example repeatedly maps, touches, and unmaps a 16-page range
that crosses a 512-page leaf-table boundary:

    anemone-bench --case vm.map_lifecycle.cross_leaf --vm-pages 16 \
        --vm-iterations 1000 --vm-leaf-span-pages 512 --repeat 5

The `vm.protect_refault.*` cases alternate read-only and writable protection,
touching every page after each transition so every iteration starts with
resident PTEs. A one-page range cannot cross a leaf-table boundary and is
rejected by the `cross_leaf` cases.

The benchmark rootfs also installs perfctl, so the same case can be wrapped as:

    perfctl run /bin/anemone-bench --case pthread.createjoin_serial1 --pthread-count 2500

The dedicated runners build that rootfs and boot a single virtual CPU:

    ./scripts/run-anemone-bench-rv64.sh
    ./scripts/run-anemone-bench-la64.sh

Both runners leave an interactive shell for repeated measurements. Use
`/bin/shutdown` when finished. The current LA64 QEMU platform halts after the
orderly shutdown path; press `Ctrl-a x` to terminate QEMU after it reports the
halt.
