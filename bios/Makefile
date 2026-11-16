FORMAT=srec #binary # or srec
ROM="AT28C256"

# Video processor for the mini68k-v2 board (pick at most one):
#   V9938=1    Yamaha V9938 driver (v9938.S)
#   TMS9918=1  TI TMS9918A driver (tms9918.S)
# REQUIRES the mini68k-v2 board (externally qualifies UARTCS with A14).
# Do not enable on v1 hardware — address decode will collide with DUART.
V9938 ?=
TMS9918 ?=
ASDEFS =
ifeq ($(V9938)$(TMS9918),11)
$(error V9938=1 and TMS9918=1 are mutually exclusive: only one VDP can be fitted)
endif
ifeq ($(V9938),1)
ASDEFS += --defsym V9938=1 --defsym HAVE_VDP=1
endif
ifeq ($(TMS9918),1)
ASDEFS += --defsym TMS9918=1 --defsym HAVE_VDP=1
endif

all: bios.bin

bios.bin: bios.S v9938.S tms9918.S font.bin
	m68k-unknown-elf-as -c -m68000 $(ASDEFS) -a=bios.lst -o bios.o bios.S
	m68k-unknown-elf-ld -Ttext 0x00000000 --oformat $(FORMAT) -o bios.bin bios.o

# font.bin is checked in; regenerate it after editing mkfont.py.
font:
	python3 mkfont.py font.bin

burn-rom: bios.bin
	minipro -w bios.bin -s -p ${ROM}

.PHONY: all font burn-rom clean

clean:
	rm -f bios.bin bios.o bios.lst bios.h68
