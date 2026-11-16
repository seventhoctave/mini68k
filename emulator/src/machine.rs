use crate::cpu;
use crate::duart68681::Duart68681;
use crate::v9938::{VideoFrame, DEFAULT_CPU_HZ, V9938};
use crate::{clear_current_machine, install_current_machine};
use std::cell::UnsafeCell;
use std::collections::VecDeque;
use std::io::{self, Read, Write};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread;

pub const DEFAULT_ROM_SIZE: usize = 32 * 1024;
pub const DEFAULT_RAM_SIZE: usize = 32 * 1024;

const ADDRESS_SPACE_MASK: u32 = 0x000f_ffff;
const REGION_MASK: u32 = 0x0001_8000;
const REGION_SUBSELECT_MASK: u32 = 0x0000_4000;
const REGION_ROM: u32 = 0x0000_0000;
const REGION_RAM: u32 = 0x0001_0000;
const REGION_IO: u32 = 0x0001_8000;

pub const DUART_IRQ_LEVEL: u32 = cpu::IRQ_2;
pub const DEFAULT_CPU_CLOCK_HZ: u64 = DEFAULT_CPU_HZ;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MachineModel {
    V1,
    V2,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VideoOutput {
    Headless,
    Window,
}

#[derive(Debug, Clone)]
pub struct MachineConfig {
    pub rom: Vec<u8>,
    pub ram_size: usize,
    pub trace: bool,
    pub echo_stdout: bool,
    pub start_stdin_thread: bool,
    pub machine_model: MachineModel,
    pub video_output: VideoOutput,
    /// CPU clock, used to pace V9938 video timing.
    pub cpu_hz: u64,
    /// 68k interrupt level wired to the V9938 /INT pin. `None` matches the
    /// v2 board, where /INT is left unconnected.
    pub vdp_irq_level: Option<u32>,
}

impl MachineConfig {
    pub fn new(rom: Vec<u8>) -> Self {
        Self {
            rom,
            ram_size: DEFAULT_RAM_SIZE,
            trace: false,
            echo_stdout: false,
            start_stdin_thread: false,
            machine_model: MachineModel::V1,
            video_output: VideoOutput::Headless,
            cpu_hz: DEFAULT_CPU_CLOCK_HZ,
            vdp_irq_level: None,
        }
    }
}

#[derive(Debug)]
pub(crate) struct MachineState {
    rom: Vec<u8>,
    ram: Vec<u8>,
    model: MachineModel,
    duart: Duart68681,
    vdp: Option<V9938>,
    vdp_irq_level: Option<u32>,
    current_fc: u8,
    trace: bool,
    echo_stdout: bool,
    booted: bool,
    total_cycles: u64,
    pending_input: VecDeque<u8>,
    stdin_rx: Option<Receiver<u8>>,
    output: Vec<u8>,
}

#[derive(Debug)]
pub struct Machine {
    state: UnsafeCell<MachineState>,
}

impl Machine {
    pub fn new(config: MachineConfig) -> Self {
        let stdin_rx = if config.start_stdin_thread {
            Some(spawn_input_thread())
        } else {
            None
        };

        let _video_output = config.video_output;
        Self {
            state: UnsafeCell::new(MachineState {
                rom: crate::rom::normalize_rom(config.rom, DEFAULT_ROM_SIZE),
                ram: vec![0; config.ram_size],
                model: config.machine_model,
                duart: Duart68681::new(),
                vdp: (config.machine_model == MachineModel::V2)
                    .then(|| V9938::with_cpu_hz(config.cpu_hz)),
                vdp_irq_level: config.vdp_irq_level.filter(|level| (1..=7).contains(level)),
                current_fc: 0,
                trace: config.trace,
                echo_stdout: config.echo_stdout,
                booted: false,
                total_cycles: 0,
                pending_input: VecDeque::new(),
                stdin_rx,
                output: Vec::new(),
            }),
        }
    }

    pub fn boot(&mut self) {
        install_current_machine(self.state.get());
        cpu::initialize();

        {
            let state = self.state_mut();
            state.current_fc = 0;
            state.total_cycles = 0;
            state.reset_peripherals();
            state.booted = true;
        }

        cpu::reset();
        self.state_mut().refresh_irq();
    }

    pub fn run(&mut self, max_cycles: Option<u64>) -> u64 {
        if !self.state_mut().booted {
            self.boot();
        }

        match max_cycles {
            Some(limit) => {
                self.run_for_cycles(limit);
            }
            None => loop {
                self.run_slice(512);
            },
        }

        self.state_mut().total_cycles
    }

    pub fn run_for_cycles(&mut self, cycles: u64) -> u64 {
        if !self.state_mut().booted {
            self.boot();
        }

        let deadline = self.state_mut().total_cycles.saturating_add(cycles);
        while self.state_mut().total_cycles < deadline {
            let remaining = deadline - self.state_mut().total_cycles;
            let slice = remaining.min(512).min(i32::MAX as u64) as i32;
            self.run_slice(slice);
        }

        self.state_mut().total_cycles
    }

    pub fn run_until_output_contains(&mut self, needle: &str, max_additional_cycles: u64) -> bool {
        if !self.state_mut().booted {
            self.boot();
        }

        let deadline = self
            .state_mut()
            .total_cycles
            .saturating_add(max_additional_cycles);
        while self.state_mut().total_cycles < deadline {
            if self.state_ref().output_as_string().contains(needle) {
                return true;
            }
            self.run_slice(512);
        }

        self.state_ref().output_as_string().contains(needle)
    }

    pub fn enqueue_input_text(&mut self, text: &str) {
        self.state_mut().enqueue_input_text(text);
    }

    pub fn output_as_string(&self) -> String {
        self.state_ref().output_as_string()
    }

    pub fn video_frame(&self) -> Option<VideoFrame> {
        self.state_ref().video_frame()
    }

    pub fn vdp(&self) -> Option<&V9938> {
        self.state_ref().vdp.as_ref()
    }

    /// Read ROM or RAM without bus side effects (I/O reads return 0xff).
    pub fn peek_memory(&self, address: u32) -> u8 {
        let state = self.state_ref();
        let physical = MachineState::physical_address(address);
        let offset = (physical & 0x7fff) as usize;
        match physical & REGION_MASK {
            REGION_ROM => state.rom.get(offset).copied().unwrap_or(0xff),
            REGION_RAM => state.ram.get(offset).copied().unwrap_or(0xff),
            _ => 0xff,
        }
    }

    pub fn total_cycles(&self) -> u64 {
        self.state_ref().total_cycles
    }

    pub fn machine_model(&self) -> MachineModel {
        self.state_ref().model
    }

    fn state_mut(&self) -> &mut MachineState {
        unsafe { &mut *self.state.get() }
    }

    fn state_ref(&self) -> &MachineState {
        unsafe { &*self.state.get() }
    }

    fn run_slice(&self, cycles: i32) -> i32 {
        {
            let state = self.state_mut();
            state.pump_console_input();
            state.refresh_irq();
        }

        let used = cpu::execute(cycles.max(1));

        {
            let state = self.state_mut();
            if used > 0 {
                state.total_cycles = state.total_cycles.saturating_add(used as u64);
                if let Some(vdp) = state.vdp.as_mut() {
                    vdp.tick(used as u64);
                }
            }
            state.pump_console_input();
            state.refresh_irq();
        }

        used
    }
}

impl Drop for Machine {
    fn drop(&mut self) {
        clear_current_machine(self.state.get());
    }
}

impl MachineState {
    pub(crate) fn trace_enabled(&self) -> bool {
        self.trace
    }

    pub(crate) fn read8(&mut self, address: u32) -> u8 {
        let value = self.read8_raw(address);
        if self.trace {
            eprintln!(
                "rd8  fc={:03b} addr={address:08x} phys={:05x} -> {value:02x}",
                self.current_fc,
                Self::physical_address(address)
            );
        }
        value
    }

    pub(crate) fn read16(&mut self, address: u32) -> u16 {
        let value =
            (u16::from(self.read8_raw(address)) << 8) | u16::from(self.read8_raw(address + 1));
        if self.trace {
            eprintln!(
                "rd16 fc={:03b} addr={address:08x} phys={:05x} -> {value:04x}",
                self.current_fc,
                Self::physical_address(address)
            );
        }
        value
    }

    pub(crate) fn read32(&mut self, address: u32) -> u32 {
        let value = (u32::from(self.read8_raw(address)) << 24)
            | (u32::from(self.read8_raw(address + 1)) << 16)
            | (u32::from(self.read8_raw(address + 2)) << 8)
            | u32::from(self.read8_raw(address + 3));
        if self.trace {
            eprintln!(
                "rd32 fc={:03b} addr={address:08x} phys={:05x} -> {value:08x}",
                self.current_fc,
                Self::physical_address(address)
            );
        }
        value
    }

    pub(crate) fn read_disassembler8(&mut self, address: u32) -> u8 {
        self.read8_raw(address)
    }

    pub(crate) fn read_disassembler16(&mut self, address: u32) -> u16 {
        (u16::from(self.read8_raw(address)) << 8) | u16::from(self.read8_raw(address + 1))
    }

    pub(crate) fn read_disassembler32(&mut self, address: u32) -> u32 {
        (u32::from(self.read8_raw(address)) << 24)
            | (u32::from(self.read8_raw(address + 1)) << 16)
            | (u32::from(self.read8_raw(address + 2)) << 8)
            | u32::from(self.read8_raw(address + 3))
    }

    pub(crate) fn write8(&mut self, address: u32, value: u8) {
        if self.trace {
            eprintln!(
                "wr8  fc={:03b} addr={address:08x} phys={:05x} <- {value:02x}",
                self.current_fc,
                Self::physical_address(address)
            );
        }
        self.write8_raw(address, value);
    }

    pub(crate) fn write16(&mut self, address: u32, value: u16) {
        if self.trace {
            eprintln!(
                "wr16 fc={:03b} addr={address:08x} phys={:05x} <- {value:04x}",
                self.current_fc,
                Self::physical_address(address)
            );
        }
        self.write8_raw(address, (value >> 8) as u8);
        self.write8_raw(address + 1, value as u8);
    }

    pub(crate) fn write32(&mut self, address: u32, value: u32) {
        if self.trace {
            eprintln!(
                "wr32 fc={:03b} addr={address:08x} phys={:05x} <- {value:08x}",
                self.current_fc,
                Self::physical_address(address)
            );
        }
        self.write8_raw(address, (value >> 24) as u8);
        self.write8_raw(address + 1, (value >> 16) as u8);
        self.write8_raw(address + 2, (value >> 8) as u8);
        self.write8_raw(address + 3, value as u8);
    }

    pub(crate) fn set_fc(&mut self, new_fc: u32) {
        self.current_fc = (new_fc & 0x07) as u8;
    }

    pub(crate) fn reset_peripherals(&mut self) {
        self.duart.reset();
        if let Some(vdp) = self.vdp.as_mut() {
            vdp.reset();
        }
        self.refresh_irq();
    }

    pub(crate) fn int_ack(&mut self, level: i32) -> i32 {
        if level == DUART_IRQ_LEVEL as i32 && self.duart.irq_asserted() {
            i32::from(self.duart.ivr())
        } else if self.vdp_irq_level.map(|l| l as i32) == Some(level) && self.vdp_irq_asserted() {
            cpu::INT_ACK_AUTOVECTOR
        } else {
            cpu::INT_ACK_SPURIOUS
        }
    }

    fn vdp_irq_asserted(&self) -> bool {
        self.vdp.as_ref().is_some_and(V9938::irq_asserted)
    }

    pub(crate) fn output_as_string(&self) -> String {
        String::from_utf8_lossy(&self.output).into_owned()
    }

    pub(crate) fn video_frame(&self) -> Option<VideoFrame> {
        self.vdp.as_ref().map(V9938::frame)
    }

    pub(crate) fn enqueue_input_text(&mut self, text: &str) {
        for byte in text.bytes() {
            if byte == b'\n' {
                self.pending_input.push_back(b'\r');
            } else {
                self.pending_input.push_back(byte);
            }
        }
    }

    pub(crate) fn pump_console_input(&mut self) {
        loop {
            let Some(rx) = self.stdin_rx.as_ref() else {
                break;
            };

            match rx.try_recv() {
                Ok(byte) => self.pending_input.push_back(byte),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    self.stdin_rx = None;
                    break;
                }
            }
        }

        while let Some(byte) = self.pending_input.pop_front() {
            self.duart.enqueue_rx(byte);
        }
    }

    pub(crate) fn refresh_irq(&mut self) {
        let duart = if self.duart.irq_asserted() {
            DUART_IRQ_LEVEL
        } else {
            cpu::IRQ_NONE
        };
        let vdp = match self.vdp_irq_level {
            Some(level) if self.vdp_irq_asserted() => level,
            _ => cpu::IRQ_NONE,
        };
        cpu::set_irq(duart.max(vdp));
    }

    pub(crate) fn physical_address(address: u32) -> u32 {
        address & ADDRESS_SPACE_MASK
    }

    fn read8_raw(&mut self, address: u32) -> u8 {
        let physical = Self::physical_address(address);
        let offset = (physical & 0x7fff) as usize;

        match physical & REGION_MASK {
            REGION_ROM => self.rom.get(offset).copied().unwrap_or(0xff),
            REGION_RAM => self.ram.get(offset).copied().unwrap_or(0xff),
            REGION_IO if self.data_space_selected() => self.read_io8(physical),
            _ => 0xff,
        }
    }

    fn write8_raw(&mut self, address: u32, value: u8) {
        let physical = Self::physical_address(address);
        let offset = (physical & 0x7fff) as usize;

        match physical & REGION_MASK {
            REGION_RAM => {
                if let Some(cell) = self.ram.get_mut(offset) {
                    *cell = value;
                }
            }
            REGION_IO if self.data_space_selected() => self.write_io8(physical, value),
            _ => {}
        }
    }

    fn read_io8(&mut self, physical: u32) -> u8 {
        if self.model == MachineModel::V2 && physical & REGION_SUBSELECT_MASK != 0 {
            let value = self
                .vdp
                .as_mut()
                .map(|vdp| vdp.read_port((physical & 0x06) as u8))
                .unwrap_or(0xff);
            self.refresh_irq();
            value
        } else {
            let value = self.duart.read_register((physical & 0x1e) as u8);
            self.refresh_irq();
            value
        }
    }

    fn write_io8(&mut self, physical: u32, value: u8) {
        if self.model == MachineModel::V2 && physical & REGION_SUBSELECT_MASK != 0 {
            if let Some(vdp) = self.vdp.as_mut() {
                vdp.write_port((physical & 0x06) as u8, value);
            }
            self.refresh_irq();
        } else {
            if let Some(output) = self.duart.write_register((physical & 0x1e) as u8, value) {
                self.emit_output(output);
            }
            self.refresh_irq();
        }
    }

    fn emit_output(&mut self, byte: u8) {
        self.output.push(byte);
        if self.echo_stdout {
            let mut stdout = io::stdout().lock();
            let _ = stdout.write_all(&[byte]);
            let _ = stdout.flush();
        }
    }

    fn data_space_selected(&self) -> bool {
        self.current_fc & 0x03 == 0x01
    }
}

fn spawn_input_thread() -> Receiver<u8> {
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let stdin = io::stdin();
        let mut handle = stdin.lock();
        let mut buffer = [0u8; 1];

        loop {
            match handle.read(&mut buffer) {
                Ok(0) => break,
                Ok(_) => {
                    let byte = if buffer[0] == b'\n' { b'\r' } else { buffer[0] };
                    if tx.send(byte).is_err() {
                        break;
                    }
                }
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(_) => break,
            }
        }
    });
    rx
}
