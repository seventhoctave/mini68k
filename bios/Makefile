all: bios.S
	m68k-unknown-elf-as -c -m68000 -a=bios.lst -o bios.o bios.S
	m68k-unknown-elf-ld -Ttext 0x00000000 --oformat srec -o bios.h68 bios.o

burn-rom: bios.bin
	minipro -w bios.bin -s -p "SST27SF512 @DIP28"

clean:
	rm -f bios.bin bios.o bios.lst bios.h68
