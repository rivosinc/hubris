// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

#![no_std]
#![no_main]

use tock_registers::interfaces::{ReadWriteable, Readable, Writeable};
use tock_registers::registers::{ReadWrite, ReadOnly};
use tock_registers::{register_bitfields, register_structs};

pub struct SifiveUart {
    regs: &'static SifiveUartRegs,
}

impl SifiveUart {
    /// Build a new Sifive Uart.
    /// # Safety
    /// you must ensure the base_addr is a valid address.
    pub unsafe fn new(base_addr: u32) -> Self {
        let regs: &'static SifiveUartRegs =
            &*(base_addr as *mut u32 as *mut SifiveUartRegs);
        Self { regs }
    }

    //
    // Functions to write the control register
    //
    pub fn tx_enable(&mut self, state: bool) {
        self.regs.txctrl.modify(TxCtrl::TXEN.val(state as u32));
    }

    pub fn rx_enable(&mut self, state: bool) {
        self.regs.rxctrl.modify(RxCtrl::RXEN.val(state as u32));
    }

    //
    // Functions to check the fifo status
    //
    pub fn tx_full(&self) -> bool {
        self.regs.txdata.read(TxData::FULL) == 1
    }

    pub fn rx_empty(&self) -> bool {
        self.regs.rxdata.read(RxData::EMPTY) == 1
    }

    //
    pub fn set_baud_rate(&mut self, clk_freq_hz: u64, baud: u64) {
        self.regs.divisor.modify(Div::DIV.val((clk_freq_hz / baud - 1) as u32));
    }

    // Waits until the the fifo has room, then push `c` to it
    pub fn poll_out_char(&mut self, c: char) {
        if self.tx_full() {
            return;
        }
        self.regs.txdata.write(TxData::DATA.val(c as u32));
    }

    pub fn write_buf(&mut self, buf: &[u8]) {
        for c in buf {
            while self.tx_full() {
                continue;
            }
            self.regs.txdata.write(TxData::DATA.val(*c as u32));
        }
    }
}

impl core::fmt::Write for SifiveUart {
    /// This will allow writing out to the uart with `format!`
    fn write_str(&mut self, s: &str) -> Result<(), core::fmt::Error> {
        for c in s.chars() {
            self.poll_out_char(c);
        }
        Ok(())
    }
}

register_structs! {
    SifiveUartRegs {
        (0x00 => txdata: ReadWrite<u32, TxData::Register>),
        (0x04 => rxdata: ReadWrite<u32, RxData::Register>),
        (0x08 => txctrl: ReadWrite<u32, TxCtrl::Register>),
        (0x0c => rxctrl: ReadWrite<u32, RxCtrl::Register>),
        (0x10 => ie: ReadWrite<u32, Interrupt::Register>),
        (0x14 => ip: ReadOnly<u32, Interrupt::Register>),
        (0x18 => divisor: ReadWrite<u32, Div::Register>),
        (0x1C => @END),
    }
}

register_bitfields![u32,
    TxData [
        DATA OFFSET(0) NUMBITS(8) [],
        FULL OFFSET(31) NUMBITS(1) [],
    ],
    RxData [
        DATA OFFSET(0) NUMBITS(8) [],
        EMPTY OFFSET(31) NUMBITS(1) [],
    ],
    TxCtrl [
        TXEN OFFSET(0) NUMBITS(1) [],
        NSTOP OFFSET(1) NUMBITS(1) [
            OneStopBit = 0,
            TwoStopBits = 1
        ],
        TXCNT OFFSET(16) NUMBITS(3) [],
    ],
    RxCtrl [
        RXEN OFFSET(0) NUMBITS(1) [],
        RXCNT OFFSET(16) NUMBITS(3) [],
    ],
    Interrupt [
        TXWM OFFSET(0) NUMBITS(1) [],
        RXWM OFFSET(1) NUMBITS(1) [],
    ],
    Div [
        DIV OFFSET(0) NUMBITS(16) [],
    ],
];
