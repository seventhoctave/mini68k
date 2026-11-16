# Demo programs for the mini68k v2 video BIOS.
# Each builds to an S-record file linked to run from RAM at 0x10000:
#   emulator:  --load demos/bounce.s28 --type 'J 10000\r'
#   hardware:  send the .s28 file to the monitor, then type J 10000
AS = m68k-unknown-elf-as
LD = m68k-unknown-elf-ld
LOAD_ADDR = 0x10000

DEMOS = bounce.s28

all: $(DEMOS)

%.o: %.S
	$(AS) -m68000 -o $@ $<

%.s28: %.o
	$(LD) -Ttext $(LOAD_ADDR) --oformat srec -o $@ $<

clean:
	rm -f *.o *.s28

.PHONY: all clean
