//! The V9938 /INT pin reaches the CPU only when the machine is configured
//! to wire it (`--vdp-irq`); the v2 board as built leaves it unconnected.

use mini68k_emu::machine::{Machine, MachineConfig, MachineModel};
use std::sync::Mutex;

/// Musashi is a single global CPU, so machines must not run concurrently.
static CPU_LOCK: Mutex<()> = Mutex::new(());

/// Hand-assembled 68000 program:
///   reset:   move.b #$60,($C002).w   ; R1 = display on + IE0
///            move.b #$81,($C002).w
///            move.w #$2000,sr        ; unmask interrupts
///   loop:    bra.s  loop
///   level 5: move.b ($C002).w,d0     ; read S#0, clears F / ack
///            addq.b #1,$00010000     ; count vblank interrupts in RAM
///            rte
fn vblank_counter_rom() -> Vec<u8> {
    let mut rom = vec![0xffu8; 32 * 1024];
    let mut put = |address: usize, bytes: &[u8]| {
        rom[address..address + bytes.len()].copy_from_slice(bytes);
    };
    put(0x000, &0x0001_7e00u32.to_be_bytes()); // SSP
    put(0x004, &0x0000_0400u32.to_be_bytes()); // PC
    put(0x074, &0x0000_0500u32.to_be_bytes()); // level 5 autovector
    put(
        0x400,
        &[
            0x11, 0xfc, 0x00, 0x60, 0xc0, 0x02, // move.b #$60,($C002).w
            0x11, 0xfc, 0x00, 0x81, 0xc0, 0x02, // move.b #$81,($C002).w
            0x46, 0xfc, 0x20, 0x00, // move.w #$2000,sr
            0x60, 0xfe, // bra.s *
        ],
    );
    put(
        0x500,
        &[
            0x10, 0x38, 0xc0, 0x02, // move.b ($C002).w,d0
            0x52, 0x39, 0x00, 0x01, 0x00, 0x00, // addq.b #1,$00010000
            0x4e, 0x73, // rte
        ],
    );
    rom
}

fn count_vblanks(vdp_irq_level: Option<u32>, frames: u64) -> u8 {
    let _guard = CPU_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let mut config = MachineConfig::new(vblank_counter_rom());
    config.machine_model = MachineModel::V2;
    config.vdp_irq_level = vdp_irq_level;
    let mut machine = Machine::new(config);
    let cycles_per_frame = config_cycles_per_frame(&machine);
    machine.run_for_cycles(cycles_per_frame * frames + cycles_per_frame / 2);
    machine.peek_memory(0x10000)
}

fn config_cycles_per_frame(machine: &Machine) -> u64 {
    let vdp = machine.vdp().expect("v2 has a VDP");
    (vdp.cpu_cycles_per_line() * f64::from(vdp.total_lines())).ceil() as u64
}

#[test]
fn vblank_interrupt_is_delivered_once_per_frame_when_wired() {
    assert_eq!(count_vblanks(Some(5), 3), 3);
}

#[test]
fn vblank_interrupt_is_not_delivered_when_int_is_unconnected() {
    assert_eq!(count_vblanks(None, 3), 0);
}
