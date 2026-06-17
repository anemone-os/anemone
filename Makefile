all:
	make build

build:
	bash ./build_all.sh

local: 
	make local_build 
	make local_test

local_build:
	docker exec -u root -w /workspaces/anemone gallant_lamarr make build
	bash ./rcopy.sh
	mkdir -p etc

local_test:
	just xtask qemu --platform pre-test-rv64 --image kernel-rv --emulator /opt/qemu-9.2.1/bin/qemu-system-riscv64 | tee etc/log-rv.log
	just xtask qemu --platform pre-test-la64 --image kernel-la --emulator /opt/qemu-9.2.1/bin/qemu-system-loongarch64 | tee etc/log-la.log

	@echo PASS COUNT ON RISC-V:
	@cat etc/log-rv.log | grep "TPASS" | wc -l
	@echo PASS COUNT ON LA:
	@cat etc/log-la.log | grep "TPASS" | wc -l

.PHONY: local_test local_build build