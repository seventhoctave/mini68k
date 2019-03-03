FORMAT=binary # or srec
ROM="AT28C256"

all: bios.bin

bios.bin: bios.S
	m68k-unknown-elf-as -c -m68000 -a=bios.lst -o bios.o bios.S
	m68k-unknown-elf-ld -Ttext 0x00000000 --oformat $(FORMAT) -o bios.bin bios.o

burn-rom: bios.bin
	minipro -w bios.bin -s -p ${ROM}

clean:
	rm -f bios.bin bios.o bios.lst bios.h68
