FORMAT=srec #binary # or srec
ROM="AT28C256"

# Set V9938=1 to build the V9938 video driver into the ROM.
# REQUIRES the mini68k-v2 board (externally qualifies UARTCS with A14).
# Do not enable on v1 hardware — address decode will collide with DUART.
V9938 ?=
ASDEFS =
ifeq ($(V9938),1)
ASDEFS += --defsym V9938=1
endif

all: bios.bin

bios.bin: bios.S v9938.S font.bin
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
