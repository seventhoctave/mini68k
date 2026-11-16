use std::collections::VecDeque;

pub const REG_MRA: u8 = 0x00;
pub const REG_SRA_CSRA: u8 = 0x02;
pub const REG_CRA: u8 = 0x04;
pub const REG_RHRA_THRA: u8 = 0x06;
pub const REG_ISR_IMR: u8 = 0x0a;
pub const REG_IVR: u8 = 0x18;

pub const ISR_RXRDYA: u8 = 1 << 1;
pub const ISR_TXRDYA: u8 = 1 << 0;

const CRA_RESET_MR_POINTER: u8 = 0x10;
const CRA_RESET_RECEIVER: u8 = 0x20;
const CRA_RESET_TRANSMITTER: u8 = 0x30;
const CRA_DISABLE_TX: u8 = 0x08;
const CRA_ENABLE_TX: u8 = 0x04;
const CRA_DISABLE_RX: u8 = 0x02;
const CRA_ENABLE_RX: u8 = 0x01;

const SRA_RX_READY: u8 = 0x01;
const SRA_TX_READY: u8 = 0x04;
const SRA_TX_EMPTY: u8 = 0x08;

#[derive(Debug, Default)]
pub struct Duart68681 {
    mr1a: u8,
    mr2a: u8,
    mode_pointer_a: u8,
    csra: u8,
    imr: u8,
    ivr: u8,
    rx_enabled_a: bool,
    tx_enabled_a: bool,
    tx_ready_a: bool,
    rx_fifo_a: VecDeque<u8>,
}

impl Duart68681 {
    pub fn new() -> Self {
        let mut duart = Self::default();
        duart.reset();
        duart
    }

    pub fn reset(&mut self) {
        self.mr1a = 0;
        self.mr2a = 0;
        self.mode_pointer_a = 0;
        self.csra = 0;
        self.imr = 0;
        self.ivr = 0x0f;
        self.rx_enabled_a = false;
        self.tx_enabled_a = false;
        self.tx_ready_a = true;
        self.rx_fifo_a.clear();
    }

    pub fn enqueue_rx(&mut self, byte: u8) {
        self.rx_fifo_a.push_back(byte);
    }

    pub fn irq_asserted(&self) -> bool {
        self.isr() & self.imr != 0
    }

    pub fn isr(&self) -> u8 {
        let mut isr = 0u8;
        if self.rx_enabled_a && !self.rx_fifo_a.is_empty() {
            isr |= ISR_RXRDYA;
        }
        if self.tx_enabled_a && self.tx_ready_a {
            isr |= ISR_TXRDYA;
        }
        isr
    }

    pub fn ivr(&self) -> u8 {
        self.ivr
    }

    pub fn read_register(&mut self, register: u8) -> u8 {
        match register & 0x1e {
            REG_MRA => {
                if self.mode_pointer_a == 0 {
                    self.mr1a
                } else {
                    self.mr2a
                }
            }
            REG_SRA_CSRA => self.status_a(),
            REG_RHRA_THRA => self.rx_fifo_a.pop_front().unwrap_or(0),
            REG_ISR_IMR => self.isr(),
            REG_IVR => self.ivr,
            _ => 0,
        }
    }

    pub fn write_register(&mut self, register: u8, value: u8) -> Option<u8> {
        match register & 0x1e {
            REG_MRA => {
                if self.mode_pointer_a == 0 {
                    self.mr1a = value;
                    self.mode_pointer_a = 1;
                } else {
                    self.mr2a = value;
                    self.mode_pointer_a = 0;
                }
                None
            }
            REG_SRA_CSRA => {
                self.csra = value;
                None
            }
            REG_CRA => {
                self.apply_command_a(value);
                None
            }
            REG_RHRA_THRA => {
                self.tx_ready_a = true;
                Some(value)
            }
            REG_ISR_IMR => {
                self.imr = value;
                None
            }
            REG_IVR => {
                self.ivr = value;
                None
            }
            _ => None,
        }
    }

    fn apply_command_a(&mut self, command: u8) {
        match command & 0x70 {
            CRA_RESET_MR_POINTER => self.mode_pointer_a = 0,
            CRA_RESET_RECEIVER => self.rx_fifo_a.clear(),
            CRA_RESET_TRANSMITTER => self.tx_ready_a = true,
            _ => {}
        }

        if command & CRA_DISABLE_TX != 0 {
            self.tx_enabled_a = false;
        }
        if command & CRA_ENABLE_TX != 0 {
            self.tx_enabled_a = true;
            self.tx_ready_a = true;
        }
        if command & CRA_DISABLE_RX != 0 {
            self.rx_enabled_a = false;
        }
        if command & CRA_ENABLE_RX != 0 {
            self.rx_enabled_a = true;
        }
    }

    fn status_a(&self) -> u8 {
        let mut status = 0u8;
        if self.rx_enabled_a && !self.rx_fifo_a.is_empty() {
            status |= SRA_RX_READY;
        }
        if self.tx_ready_a {
            status |= SRA_TX_READY | SRA_TX_EMPTY;
        }
        status
    }
}

#[cfg(test)]
mod tests {
    use super::{Duart68681, ISR_RXRDYA, ISR_TXRDYA, REG_CRA, REG_IVR, REG_MRA, REG_RHRA_THRA};

    const REG_IMR: u8 = super::REG_ISR_IMR;

    #[test]
    fn tx_interrupt_tracks_enable_state() {
        let mut duart = Duart68681::new();
        duart.write_register(REG_IMR, ISR_TXRDYA);
        assert!(!duart.irq_asserted());

        duart.write_register(REG_CRA, 0x04);
        assert!(duart.irq_asserted());

        let emitted = duart.write_register(REG_RHRA_THRA, b'A');
        assert_eq!(emitted, Some(b'A'));
        assert!(duart.irq_asserted());

        duart.write_register(REG_CRA, 0x08);
        assert!(!duart.irq_asserted());
    }

    #[test]
    fn rx_interrupt_clears_after_byte_is_read() {
        let mut duart = Duart68681::new();
        duart.write_register(REG_IMR, ISR_RXRDYA);
        duart.write_register(REG_CRA, 0x01);
        duart.enqueue_rx(b'Z');

        assert_eq!(duart.isr() & ISR_RXRDYA, ISR_RXRDYA);
        assert_eq!(duart.read_register(REG_RHRA_THRA), b'Z');
        assert_eq!(duart.isr() & ISR_RXRDYA, 0);
    }

    #[test]
    fn register_programming_round_trips() {
        let mut duart = Duart68681::new();
        duart.write_register(REG_MRA, 0x13);
        duart.write_register(REG_MRA, 0x07);
        duart.write_register(REG_IVR, 0x40);

        assert_eq!(duart.read_register(REG_IVR), 0x40);
    }
}
