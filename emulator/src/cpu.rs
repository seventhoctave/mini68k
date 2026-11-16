use std::ffi::{c_char, c_int, c_uint, c_void, CStr};

pub const CPU_TYPE_68000: u32 = 1;
pub const IRQ_NONE: u32 = 0;
pub const IRQ_2: u32 = 2;
pub const INT_ACK_SPURIOUS: i32 = 0xffff_fffe_u32 as i32;
pub const INT_ACK_AUTOVECTOR: i32 = 0xffff_ffff_u32 as i32;

unsafe extern "C" {
    fn m68k_set_cpu_type(cpu_type: c_uint);
    fn m68k_init();
    fn m68k_pulse_reset();
    fn m68k_execute(num_cycles: c_int) -> c_int;
    fn m68k_set_irq(level: c_uint);
    fn m68k_disassemble(buffer: *mut c_char, pc: c_uint, cpu_type: c_uint) -> c_uint;
    fn m68k_get_reg(context: *mut c_void, reg: c_int) -> c_uint;
}

const REG_PC: c_int = 16;
const REG_SR: c_int = 17;

pub fn initialize() {
    unsafe {
        m68k_init();
        m68k_set_cpu_type(CPU_TYPE_68000);
    }
}

pub fn reset() {
    unsafe {
        m68k_pulse_reset();
    }
}

pub fn execute(cycles: i32) -> i32 {
    unsafe { m68k_execute(cycles) }
}

pub fn set_irq(level: u32) {
    unsafe {
        m68k_set_irq(level);
    }
}

pub fn current_pc() -> u32 {
    unsafe { m68k_get_reg(std::ptr::null_mut(), REG_PC) }
}

pub fn current_sr() -> u16 {
    unsafe { m68k_get_reg(std::ptr::null_mut(), REG_SR) as u16 }
}

pub fn disassemble(pc: u32) -> String {
    let mut buffer = [0i8; 256];
    unsafe {
        m68k_disassemble(buffer.as_mut_ptr(), pc, CPU_TYPE_68000);
        CStr::from_ptr(buffer.as_ptr())
            .to_string_lossy()
            .into_owned()
    }
}
