// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

#![no_std]

use tock_registers::interfaces::{Readable, Writeable};
use tock_registers::registers::{ReadWrite, WriteOnly};
use tock_registers::{register_bitfields, register_structs};

use drv_timer_api::TimerApi;

pub struct QemuMainstreamIbexTimer {
    regs: &'static QemuMainstreamIbexTimerRegs,
}

impl QemuMainstreamIbexTimer {
    /// Build a new Ibex timer.
    /// # Safety
    /// you must ensure the base_addr is a valid address.
    pub unsafe fn new(base_addr: u32) -> Self {
        let regs: &'static QemuMainstreamIbexTimerRegs =
            &*(base_addr as *mut u32 as *mut QemuMainstreamIbexTimerRegs);
        Self { regs }
    }

    /// Enable the timer and interrupts.
    pub fn init(&self, prescale: u16, step: u8) {
        self.regs.config_0.write(
            Config0::PRESCALE.val(prescale as u32)
                + Config0::STEP.val(step as u32),
        );

        self.regs.control.write(Control::ACTIVE_0::SET);
        self.regs.intr_enable.write(IntrEnable::IE_0::SET);
    }
}

impl TimerApi for QemuMainstreamIbexTimer {
    fn get_time(&self) -> u64 {
        // Guard against rollover when reading 64bits value on 32bits arch.
        loop {
            let hi = self.regs.timer_value_upper_0.get();
            let lo = self.regs.timer_value_lower_0.get();
            if hi == self.regs.timer_value_upper_0.get() {
                break ((hi as u64) << 32) | lo as u64;
            }
        }
    }

    fn set_timer(&mut self, value: u64) {
        self.regs.compare_upper_0.set(0xFFFFFFFF);
        self.regs.compare_lower_0.set((value & 0xFFFFFFFF) as u32);
        self.regs.compare_upper_0.set((value >> 32) as u32);
    }

    fn increase_timer(&mut self, delta: u64) {
        let mut value: u64 = (self.regs.compare_upper_0.get() as u64) << 32;
        value += self.regs.compare_lower_0.get() as u64;
        value += delta;
        self.set_timer(value);
    }
}

register_structs! {
    QemuMainstreamIbexTimerRegs {
        // Alert Test Register
        (0x0000 => alert_test: WriteOnly<u32, AlertTest::Register>),
        // Control register
        (0x0004 => control: ReadWrite<u32, Control::Register>),
        // reserved
        (0x0008 => _reserved),
        // Configuration for Hart 0
        (0x0100 => config_0: ReadWrite<u32, Config0::Register>),
        // Timer value Lower
        (0x0104 => timer_value_lower_0: ReadWrite<u32>),
        // Timer value Upper
        (0x0108 => timer_value_upper_0: ReadWrite<u32>),
        // Timer value Lower
        (0x010c => compare_lower_0: ReadWrite<u32>),
        // Timer value Upper
        (0x0110 => compare_upper_0: ReadWrite<u32>),
        // Interrupt Enable
        (0x0114 => intr_enable: ReadWrite<u32, IntrEnable::Register>),
        // Interrupt Status
        (0x0118 => intr_status: ReadWrite<u32, IntrStatus::Register>),
        // Interrupt test register
        (0x011c => intr_test: WriteOnly<u32, IntrTest::Register>),
        (0x0120 => @END),
    }
}

register_bitfields![u32,
    AlertTest [
        FATAL_FAULT OFFSET(0) NUMBITS(1) [],
    ],
    Control [
        ACTIVE_0 OFFSET(0) NUMBITS(1) [],
    ],
    IntrEnable [
        IE_0 OFFSET(0) NUMBITS(1) [],
    ],
    IntrStatus [
        IS_0 OFFSET(0) NUMBITS(1) [],
    ],
    IntrTest [
        TEST_0 OFFSET(0) NUMBITS(1) [],
    ],
    Config0 [
        PRESCALE OFFSET(0) NUMBITS(12) [],
        STEP OFFSET(16) NUMBITS(8) [],
    ],
];
