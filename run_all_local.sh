
SAVE_FNAME = $(date "+%Y-%m-%d-%H-%M-%S")

just xtask qemu --platform pre-test-rv64 --image kernel-rv --emulator /opt/qemu-9.2.1/bin/qemu-system-riscv64 | tee etc/log-rv-$SAVE_FNAME.log &
just xtask qemu --platform pre-test-la64 --image kernel-la --emulator /opt/qemu-9.2.1/bin/qemu-system-loongarch64 | tee etc/log-la-$SAVE_FNAME.log &
wait

echo PASS COUNT ON RISC-V:
cat etc/log-rv-$SAVE_FNAME.log | grep "TPASS" | wc -l
echo PASS COUNT ON LA:
cat etc/log-la-$SAVE_FNAME.log | grep "TPASS" | wc -l