all: bios.S
	m68k-unknown-elf-as -c -m68000 -o bios.o bios.S
	m68k-unknown-elf-ld -Ttext 0x00000000 --oformat binary -o bios.bin bios.o

burn-rom: bios.bin
	minipro -w bios.bin -s -p "SST27SF512 @DIP28"
