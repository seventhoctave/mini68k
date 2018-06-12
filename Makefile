all:
	m68k-unknown-elf-as -c -m68000 -o bios.o bios.S
	m68k-unknown-elf-ld -Ttext 0x00000000 --oformat binary -o bios.bin bios.o
