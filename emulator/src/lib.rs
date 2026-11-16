pub mod cpu;
pub mod duart68681;
pub mod machine;
pub mod rom;
pub mod tms9918a;
pub mod v9938;

use machine::MachineState;
use std::sync::atomic::{AtomicPtr, Ordering};

static CURRENT_MACHINE: AtomicPtr<MachineState> = AtomicPtr::new(std::ptr::null_mut());

pub(crate) fn install_current_machine(machine: *mut MachineState) {
    CURRENT_MACHINE.store(machine, Ordering::SeqCst);
}

pub(crate) fn clear_current_machine(machine: *mut MachineState) {
    let _ = CURRENT_MACHINE.compare_exchange(
        machine,
        std::ptr::null_mut(),
        Ordering::SeqCst,
        Ordering::SeqCst,
    );
}

fn with_current_machine_mut<R>(f: impl FnOnce(&mut MachineState) -> R) -> Option<R> {
    let machine = CURRENT_MACHINE.load(Ordering::SeqCst);
    if machine.is_null() {
        None
    } else {
        Some(unsafe { f(&mut *machine) })
    }
}

#[no_mangle]
pub extern "C" fn mini68k_read_memory_8(address: u32) -> u32 {
    with_current_machine_mut(|machine| u32::from(machine.read8(address))).unwrap_or(0xff)
}

#[no_mangle]
pub extern "C" fn mini68k_read_memory_16(address: u32) -> u32 {
    with_current_machine_mut(|machine| u32::from(machine.read16(address))).unwrap_or(0xffff)
}

#[no_mangle]
pub extern "C" fn mini68k_read_memory_32(address: u32) -> u32 {
    with_current_machine_mut(|machine| machine.read32(address)).unwrap_or(0xffff_ffff)
}

#[no_mangle]
pub extern "C" fn mini68k_read_disassembler_8(address: u32) -> u32 {
    with_current_machine_mut(|machine| u32::from(machine.read_disassembler8(address)))
        .unwrap_or(0xff)
}

#[no_mangle]
pub extern "C" fn mini68k_read_disassembler_16(address: u32) -> u32 {
    with_current_machine_mut(|machine| u32::from(machine.read_disassembler16(address)))
        .unwrap_or(0xffff)
}

#[no_mangle]
pub extern "C" fn mini68k_read_disassembler_32(address: u32) -> u32 {
    with_current_machine_mut(|machine| machine.read_disassembler32(address)).unwrap_or(0xffff_ffff)
}

#[no_mangle]
pub extern "C" fn mini68k_write_memory_8(address: u32, value: u32) {
    let _ = with_current_machine_mut(|machine| machine.write8(address, value as u8));
}

#[no_mangle]
pub extern "C" fn mini68k_write_memory_16(address: u32, value: u32) {
    let _ = with_current_machine_mut(|machine| machine.write16(address, value as u16));
}

#[no_mangle]
pub extern "C" fn mini68k_write_memory_32(address: u32, value: u32) {
    let _ = with_current_machine_mut(|machine| machine.write32(address, value));
}

#[no_mangle]
pub extern "C" fn mini68k_int_ack(level: i32) -> i32 {
    with_current_machine_mut(|machine| machine.int_ack(level)).unwrap_or(cpu::INT_ACK_SPURIOUS)
}

#[no_mangle]
pub extern "C" fn mini68k_reset_instruction() {
    let _ = with_current_machine_mut(|machine| machine.reset_peripherals());
}

#[no_mangle]
pub extern "C" fn mini68k_set_fc(new_fc: u32) {
    let _ = with_current_machine_mut(|machine| machine.set_fc(new_fc));
}

#[no_mangle]
pub extern "C" fn mini68k_instruction_hook(pc: u32) {
    let machine = CURRENT_MACHINE.load(Ordering::SeqCst);
    if machine.is_null() {
        return;
    }

    let trace = unsafe { (&*machine).trace_enabled() };
    if !trace {
        return;
    }

    let disassembly = cpu::disassemble(pc);
    eprintln!(
        "pc={pc:08x} phys={:05x} sr={:04x} {disassembly}",
        MachineState::physical_address(pc),
        cpu::current_sr()
    );
}
