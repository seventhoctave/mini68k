DOCKER_COMPOSE ?= docker compose
ROM ?= bios/bios.bin
EMU_ARGS ?=
V9938 ?=

.PHONY: bios-docker emu-build emu-test emu-run dev-shell

bios-docker:
	$(DOCKER_COMPOSE) run --rm dev make -B -C bios FORMAT=binary V9938=$(V9938)

emu-build:
	$(DOCKER_COMPOSE) run --rm dev cargo build --release --workspace

emu-test:
	$(DOCKER_COMPOSE) run --rm dev cargo test --workspace -- --test-threads=1

emu-run: bios-docker
	@case " $(EMU_ARGS) " in \
		*" --video window "*) \
			echo "Running windowed emulator on the host so it can open a desktop window..."; \
			cargo run --release -p mini68k-emu -- --rom $(ROM) $(EMU_ARGS) ;; \
		*) \
			$(DOCKER_COMPOSE) run --rm dev cargo run --release -p mini68k-emu -- --rom $(ROM) $(EMU_ARGS) ;; \
	esac

dev-shell:
	$(DOCKER_COMPOSE) run --rm dev bash
