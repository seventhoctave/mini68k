#ifndef MINI68K_M68KCONF_H
#define MINI68K_M68KCONF_H

#define OPT_OFF 0
#define OPT_ON 1
#define OPT_SPECIFY_HANDLER 2

#ifndef M68K_COMPILE_FOR_MAME
#define M68K_COMPILE_FOR_MAME OPT_OFF
#endif

#if M68K_COMPILE_FOR_MAME == OPT_OFF

#define M68K_EMULATE_010 OPT_OFF
#define M68K_EMULATE_EC020 OPT_OFF
#define M68K_EMULATE_020 OPT_OFF
#define M68K_EMULATE_030 OPT_OFF
#define M68K_EMULATE_040 OPT_OFF

#define M68K_SEPARATE_READS OPT_OFF
#define M68K_SIMULATE_PD_WRITES OPT_OFF

#define M68K_EMULATE_INT_ACK OPT_SPECIFY_HANDLER
#define M68K_INT_ACK_CALLBACK(level) mini68k_int_ack(level)

#define M68K_EMULATE_BKPT_ACK OPT_OFF
#define M68K_EMULATE_TRACE OPT_OFF

#define M68K_EMULATE_RESET OPT_SPECIFY_HANDLER
#define M68K_RESET_CALLBACK() mini68k_reset_instruction()

#define M68K_CMPILD_HAS_CALLBACK OPT_OFF
#define M68K_RTE_HAS_CALLBACK OPT_OFF
#define M68K_TAS_HAS_CALLBACK OPT_OFF
#define M68K_ILLG_HAS_CALLBACK OPT_OFF

#define M68K_EMULATE_FC OPT_SPECIFY_HANDLER
#define M68K_SET_FC_CALLBACK(fc) mini68k_set_fc(fc)

#define M68K_MONITOR_PC OPT_OFF

#define M68K_INSTRUCTION_HOOK OPT_SPECIFY_HANDLER
#define M68K_INSTRUCTION_CALLBACK(pc) mini68k_instruction_hook(pc)

#define M68K_EMULATE_PREFETCH OPT_OFF
#define M68K_EMULATE_ADDRESS_ERROR OPT_ON

#define M68K_LOG_ENABLE OPT_OFF
#define M68K_LOG_1010_1111 OPT_OFF

#define M68K_EMULATE_PMMU OPT_OFF
#define M68K_DASM_ENABLE OPT_ON
#define M68K_DYNAMIC_INSTR_TABLES OPT_OFF
#define M68K_CYCLE_COUNTING OPT_OFF
#define M68K_BUS_ERR_ENABLE OPT_OFF
#define M68K_USE_64_BIT OPT_ON

#include "mini68k_musashi_bridge.h"

#define m68k_read_memory_8(address) mini68k_read_memory_8(address)
#define m68k_read_memory_16(address) mini68k_read_memory_16(address)
#define m68k_read_memory_32(address) mini68k_read_memory_32(address)

#define m68k_read_disassembler_8(address) mini68k_read_disassembler_8(address)
#define m68k_read_disassembler_16(address) mini68k_read_disassembler_16(address)
#define m68k_read_disassembler_32(address) mini68k_read_disassembler_32(address)

#define m68k_read_instr_16(address) mini68k_read_memory_16(address)
#define m68k_read_instr_32(address) mini68k_read_memory_32(address)

#define m68k_write_memory_8(address, value) mini68k_write_memory_8(address, value)
#define m68k_write_memory_16(address, value) mini68k_write_memory_16(address, value)
#define m68k_write_memory_32(address, value) mini68k_write_memory_32(address, value)

#endif

#endif
