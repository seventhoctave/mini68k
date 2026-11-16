DOCKER_COMPOSE ?= docker compose
ROM ?= bios/bios.bin
EMU_ARGS ?=
V9938 ?=
TMS9918 ?=

.PHONY: bios-docker emu-build emu-test emu-run dev-shell demo-docker demo-run

bios-docker:
	$(DOCKER_COMPOSE) run --rm dev make -B -C bios FORMAT=binary V9938=$(V9938) TMS9918=$(TMS9918)

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

# Demos run on the v2 video BIOS: V9938 by default, TMS9918=1 for the TMS9918A.
DEMO ?= bounce
DEMO_BIOS_FLAGS = $(if $(filter 1,$(TMS9918)),TMS9918=1,V9938=1)
DEMO_VDP = $(if $(filter 1,$(TMS9918)),tms9918a,v9938)

demo-docker:
	$(DOCKER_COMPOSE) run --rm dev make -B -C demos

demo-run: demo-docker
	$(DOCKER_COMPOSE) run --rm dev make -B -C bios FORMAT=binary $(DEMO_BIOS_FLAGS)
	cargo run --release -p mini68k-emu -- --rom $(ROM) --machine v2 --vdp $(DEMO_VDP) \
		--video window --load demos/$(DEMO).s28 --type 'J 10000\r' $(EMU_ARGS)

dev-shell:
	$(DOCKER_COMPOSE) run --rm dev bash
