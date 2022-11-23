// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

#![no_std]

use tock_registers::interfaces::{Readable, Writeable};
use tock_registers::register_structs;
use tock_registers::registers::ReadWrite;

use drv_timer_api::TimerApi;

pub struct SifiveClint {
    regs: &'static SifiveClintRegs,
}

impl SifiveClint {
    /// Build a new SiFive clint.
    /// # Safety
    /// you must ensure the base_addr is a valid address.
    pub unsafe fn new(base_addr: u32) -> Self {
        let regs: &'static SifiveClintRegs =
            &*(base_addr as *mut u32 as *mut SifiveClintRegs);
        Self { regs }
    }
}

impl TimerApi for SifiveClint {
    fn get_time(&self) -> u64 {
        // Guard against rollover when reading 64bits value on 32bits arch.
        loop {
            let hi = self.regs.value_high.get();
            let lo = self.regs.value_low.get();
            if hi == self.regs.value_high.get() {
                break ((hi as u64) << 32) | lo as u64;
            }
        }
    }

    fn set_timer(&mut self, value: u64) {
        self.regs.compare_high.set(0xFFFFFFFF);
        self.regs.compare_low.set((value & 0xFFFFFFFF) as u32);
        self.regs.compare_high.set((value >> 32) as u32);
    }

    fn increase_timer(&mut self, delta: u64) {
        let mut value: u64 = (self.regs.compare_high.get() as u64) << 32;
        value += self.regs.compare_low.get() as u64;
        value += delta;
        self.set_timer(value);
    }
}

register_structs! {
    SifiveClintRegs {
        (0x0000 => msip: ReadWrite<u32>),
        (0x0004 => _reserved),
        (0x4000 => compare_low: ReadWrite<u32>),
        (0x4004 => compare_high: ReadWrite<u32>),
        (0x4008 => _reserved2),
        (0xBFF8 => value_low: ReadWrite<u32>),
        (0xBFFC => value_high: ReadWrite<u32>),
        (0xC000 => @END),
    }
}
