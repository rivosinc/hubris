// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

#![no_std]

use tock_registers::interfaces::{ReadWriteable, Readable, Writeable};
use tock_registers::registers::ReadWrite;
use tock_registers::{register_bitfields, register_structs};

#[derive(Copy, Clone, Debug)]
pub enum IbexUartError {
    InvalidBaud,
}

pub struct IbexUart {
    regs: &'static IbexUartRegisters,
}

pub enum Parity {
    Odd = 1,
    Even = 0,
}

pub enum RxBlvl {
    Break2 = 0,
    Break4,
    Break8,
    Break16,
}

//TODO can i get this from the tock struct?
/// Number of bits of the NCO field in the CTRL register
const NCO_BITS: u8 = 16;
/// The bitmask that covers all of `NCO_BITS`
const NCO_MASK: u64 = 0xFFFF_FFFF_FFFF_FFFF >> (64 - NCO_BITS);
/// When calcutating the NCO out from the baud and system clock, you must multiply by 16
const MULT_EXP: u8 = 4;
/// the baud rate is multiplied by (2^4)*(2^NCO_BITS)
const SHIFT_AMT: u8 = NCO_BITS + MULT_EXP;

/// How accurate a baud rate needs to be to ensure correct communication
pub const BAUD_TOL: f32 = 2.5 / 100.0;

/// This driver is for https://docs.opentitan.org/hw/ip/uart/doc/
/// Currently it only supports polling outputs.  It does not support interrupts or reading
/// characters in.
impl IbexUart {
    /// # Safety
    /// `base_addr` must point to the actual ot uart mmio
    pub unsafe fn new(base_addr: u32) -> Self {
        let mut uart = IbexUart {
            regs: &*(base_addr as *mut u32 as *mut IbexUartRegisters),
        };

        uart.tx_enable(true);

        uart
    }

    /// Waits until the the fifo has room, then push `c` to it
    pub fn poll_out_char(&mut self, c: char) {
        while self.tx_full() {
            continue;
        }
        self.regs.wdata.write(WData::WDATA.val(c as u32));
    }

    pub fn write_buf(&mut self, buf: &[u8]) {
        for c in buf {
            while self.tx_full() {
                continue;
            }
            self.regs.wdata.write(WData::WDATA.val(*c as u32));
        }
    }

    //
    // Functions to write the control register
    //
    pub fn tx_enable(&mut self, state: bool) {
        self.regs.ctrl.modify(Ctrl::TX.val(state as u32));
    }

    pub fn rx_enable(&mut self, state: bool) {
        self.regs.ctrl.modify(Ctrl::RX.val(state as u32));
    }

    pub fn nf_enable(&mut self, state: bool) {
        self.regs.ctrl.modify(Ctrl::NF.val(state as u32));
    }
    pub fn slpbk_enable(&mut self, state: bool) {
        self.regs.ctrl.modify(Ctrl::SLPBK.val(state as u32));
    }
    pub fn llpbk_enable(&mut self, state: bool) {
        self.regs.ctrl.modify(Ctrl::LLPBK.val(state as u32));
    }

    pub fn set_parity(&mut self, parity: Option<Parity>) {
        if let Some(parity) = parity {
            self.regs.ctrl.modify(Ctrl::PARITY_EN.val(1));
            self.regs.ctrl.modify(Ctrl::PARITY_ODD.val(parity as u32));
        } else {
            self.regs.ctrl.modify(Ctrl::PARITY_EN.val(0));
        }
    }

    /// Just blindly compute the NCO and smack it into the CTRL register,  there is NO checking to
    /// ensure anything is the right size, or did not get "too" rounded when dividing.
    pub fn unchecked_set_baud_rate(&mut self, baud: u64, clk_freq_hz: u64) {
        // we are going to decode the final
        // nco to ensure it matches, so no point in checking if bits are dropped here.
        let div_baud: u64 = baud << SHIFT_AMT as u32;

        let nco: u64 = div_baud / clk_freq_hz;

        self.regs.ctrl.modify(Ctrl::NCO.val(nco as u32));
    }

    /// Sets the baud rate, with checking to prevent overflows and rounding errors.  This function
    /// will return the actual baud rate that the uart is configured too.
    /// See [opentitan docs](https://docs.opentitan.org/hw/ip/uart/doc/#setting-the-baud-rate) for
    /// details.
    pub fn checked_set_baud_rate(
        &mut self,
        baud: u64,
        clk_freq_hz: u64,
    ) -> Result<u64, IbexUartError> {
        // we are going to decode the final
        // nco to ensure it matches, so no point in checking if bits are dropped here.
        let div_baud: u64 = baud
            .checked_shl(SHIFT_AMT as u32)
            .ok_or(IbexUartError::InvalidBaud)?;

        let nco: u64 = div_baud / clk_freq_hz;

        // Ensure our actual baud rate will be within an acceptable range of the desired baud rate
        let actual_baud = Self::decode_nco(nco, clk_freq_hz);
        if (actual_baud as i64 - baud as i64) as f32 / baud as f32 > BAUD_TOL {
            return Err(IbexUartError::InvalidBaud);
        }

        self.regs.ctrl.modify(Ctrl::NCO.val(nco as u32));

        Ok(actual_baud)
    }

    /// Just blindly set the NCO. It is mostly use with emulation.
    pub fn set_nco(&mut self, nco: u32) {
        self.regs.ctrl.modify(Ctrl::NCO.val(nco));
    }

    pub fn get_baud_rate(&self, clk_freq_hz: u64) -> u64 {
        let nco: u32 = self.regs.ctrl.get();
        Self::decode_nco(nco as u64, clk_freq_hz)
    }

    fn decode_nco(nco: u64, clk_freq_hz: u64) -> u64 {
        let nco = nco & NCO_MASK;
        (nco * clk_freq_hz) >> SHIFT_AMT
    }

    pub fn set_rx_blvl(&mut self, lvl: RxBlvl) {
        self.regs.ctrl.modify(Ctrl::RXBLVL.val(lvl as u32));
    }

    //
    // Functions for FIFO control register
    //
    pub fn clear_rx_fifo(&mut self) {
        self.regs.fifo_ctrl.modify(FifoCtrl::RXRST.val(1));
    }

    pub fn clear_tx_fifo(&mut self) {
        self.regs.fifo_ctrl.modify(FifoCtrl::TXRST.val(1));
    }

    //
    // Functions to check the status register
    //
    pub fn tx_full(&self) -> bool {
        self.regs.status.read(Status::TXFULL) == 1
    }

    pub fn rx_full(&self) -> bool {
        self.regs.status.read(Status::RXFULL) == 1
    }

    pub fn tx_empty(&self) -> bool {
        self.regs.status.read(Status::TXEMPTY) == 1
    }

    pub fn tx_idle(&self) -> bool {
        self.regs.status.read(Status::TXIDLE) == 1
    }

    pub fn rx_idle(&self) -> bool {
        self.regs.status.read(Status::RXFULL) == 1
    }

    pub fn rx_empty(&self) -> bool {
        self.regs.status.read(Status::RXEMPTY) == 1
    }
}

impl core::fmt::Write for IbexUart {
    /// This will allow writing out to the uart with `format!`
    fn write_str(&mut self, s: &str) -> Result<(), core::fmt::Error> {
        for c in s.chars() {
            self.poll_out_char(c);
        }
        Ok(())
    }
}

register_structs! {
    pub IbexUartRegisters {
        // Interrupt State Register
        (0x0000 => intr_state: ReadWrite<u32, Intr::Register>),
        // Interrupt Enable Register
        (0x0004 => intr_enable: ReadWrite<u32, Intr::Register>),
        // Interrupt Test Register
        (0x0008 => intr_test: ReadWrite<u32, Intr::Register>),
        // Alert Test Register
        (0x000c => alert_test: ReadWrite<u32, AlertTest::Register>),
        // UART control register
        (0x0010 => ctrl: ReadWrite<u32, Ctrl::Register>),
        // UART live status register
        (0x0014 => status: ReadWrite<u32, Status::Register>),
        // UART read data
        (0x0018 => rdata: ReadWrite<u32, RData::Register>),
        // UART write data
        (0x001c => wdata: ReadWrite<u32, WData::Register>),
        // UART FIFO control register
        (0x0020 => fifo_ctrl: ReadWrite<u32, FifoCtrl::Register>),
        // UART FIFO status register
        (0x0024 => fifo_status: ReadWrite<u32, FifoStatus::Register>),
        // TX pin override control. Gives direct SW control over TX pin state
        (0x0028 => ovrd: ReadWrite<u32, Ovrd::Register>),
        // UART oversampled values
        (0x002c => val: ReadWrite<u32, Val::Register>),
        // UART RX timeout control
        (0x0030 => timeout_ctrl: ReadWrite<u32, TimeoutCtrl::Register>),
        (0x0034 => @END),
    }
}

register_bitfields![u32,
    // Common Interrupt Offsets
    Intr [
        TX_WATERMARK OFFSET(0) NUMBITS(1) [],
        RX_WATERMARK OFFSET(1) NUMBITS(1) [],
        TX_EMPTY OFFSET(2) NUMBITS(1) [],
        RX_OVERFLOW OFFSET(3) NUMBITS(1) [],
        RX_FRAME_ERR OFFSET(4) NUMBITS(1) [],
        RX_BREAK_ERR OFFSET(5) NUMBITS(1) [],
        RX_TIMEOUT OFFSET(6) NUMBITS(1) [],
        RX_PARITY_ERR OFFSET(7) NUMBITS(1) [],
    ],
    AlertTest [
        FATAL_FAULT OFFSET(0) NUMBITS(1) [],
    ],
    Ctrl [
        TX OFFSET(0) NUMBITS(1) [],
        RX OFFSET(1) NUMBITS(1) [],
        NF OFFSET(2) NUMBITS(1) [],
        SLPBK OFFSET(4) NUMBITS(1) [],
        LLPBK OFFSET(5) NUMBITS(1) [],
        PARITY_EN OFFSET(6) NUMBITS(1) [],
        PARITY_ODD OFFSET(7) NUMBITS(1) [],
        RXBLVL OFFSET(8) NUMBITS(2) [
            BREAK2 = 0,
            BREAK4 = 1,
            BREAK8 = 2,
            BREAK16 = 3,
        ],
        NCO OFFSET(16) NUMBITS(16) [],
    ],
    Status [
        TXFULL OFFSET(0) NUMBITS(1) [],
        RXFULL OFFSET(1) NUMBITS(1) [],
        TXEMPTY OFFSET(2) NUMBITS(1) [],
        TXIDLE OFFSET(3) NUMBITS(1) [],
        RXIDLE OFFSET(4) NUMBITS(1) [],
        RXEMPTY OFFSET(5) NUMBITS(1) [],
    ],
    RData [
        RDATA OFFSET(0) NUMBITS(8) [],
    ],
    WData [
        WDATA OFFSET(0) NUMBITS(8) [],
    ],
    FifoCtrl [
        RXRST OFFSET(0) NUMBITS(1) [],
        TXRST OFFSET(1) NUMBITS(1) [],
        RXILVL OFFSET(2) NUMBITS(3) [
            RXLVL1 = 0,
            RXLVL2 = 1,
            RXLVL4 = 2,
            RXLVL8 = 3,
            RXLVL16 = 4,
            RXLVL32 = 5,
            RXLVL64 = 6,
            RXLVL126 = 7,
        ],
        TXILVL OFFSET(5) NUMBITS(3) [
            TXLVL1 = 0,
            TXLVL2 = 1,
            TXLVL4 = 2,
            TXLVL8 = 3,
            TXLVL16 = 4,
            TXLVL32 = 5,
            TXLVL64 = 6,
        ],
    ],
    FifoStatus [
        TXLVL OFFSET(0) NUMBITS(8) [],
        RXLVL OFFSET(16) NUMBITS(8) [],
    ],
    Ovrd [
        TXEN OFFSET(0) NUMBITS(1) [],
        TXVAL OFFSET(1) NUMBITS(1) [],
    ],
    Val [
        RX OFFSET(0) NUMBITS(16) [],
    ],
    TimeoutCtrl [
        VAL OFFSET(0) NUMBITS(24) [],
        EN OFFSET(31) NUMBITS(1) [],
    ],
];
