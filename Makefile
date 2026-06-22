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
	bash run_all_local.sh

.PHONY: local_test local_build build