#ifndef MINI68K_MUSASHI_BRIDGE_H
#define MINI68K_MUSASHI_BRIDGE_H

#ifdef __cplusplus
extern "C" {
#endif

unsigned int mini68k_read_memory_8(unsigned int address);
unsigned int mini68k_read_memory_16(unsigned int address);
unsigned int mini68k_read_memory_32(unsigned int address);

unsigned int mini68k_read_disassembler_8(unsigned int address);
unsigned int mini68k_read_disassembler_16(unsigned int address);
unsigned int mini68k_read_disassembler_32(unsigned int address);

void mini68k_write_memory_8(unsigned int address, unsigned int value);
void mini68k_write_memory_16(unsigned int address, unsigned int value);
void mini68k_write_memory_32(unsigned int address, unsigned int value);

int mini68k_int_ack(int level);
void mini68k_reset_instruction(void);
void mini68k_set_fc(unsigned int new_fc);
void mini68k_instruction_hook(unsigned int pc);

#ifdef __cplusplus
}
#endif

#endif
