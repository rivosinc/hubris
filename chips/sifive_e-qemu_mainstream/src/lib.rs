// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

#![no_std]

use drv_chip_api::ChipApi;
use drv_sifive_clint::SifiveClint;
use drv_int_chip_api::{IntChipApi, IrqStatus, UsageError};
use drv_int_ctrl_api::IntCtrlApi;
use drv_riscv_plic::Plic;
use drv_timer_api::TimerApi;

pub struct SifiveEChip {
    timer: SifiveClint,
    plic: Plic<PLIC_PRIORITY_BITS, PLIC_MAX_IRQ, PLIC_CONTEXT_ID>,
}

impl SifiveEChip {
    pub fn new() -> Self {
        // Safety: The addresses are generated from the chip.toml file, we assume
        // it is valid
        unsafe {
            Self {
                timer: SifiveClint::new(SIFIVE_CLINT_BASE_ADDR),
                plic: Plic::new(PLIC_BASE_ADDR as usize),
            }
        }
    }

    pub fn init(&self) -> u32 {
        // No specific function to enable the timer

        // Initialize plic controller
        self.plic.init();
        for irq in PLIC_USED_IRQ {
            self.plic.set_priority(irq as usize, 1);
        }

        TIMER_KERNEL_TICK_DIVISOR
    }
}

impl TimerApi for SifiveEChip {
    fn get_time(&self) -> u64 {
        self.timer.get_time()
    }

    fn set_timer(&mut self, timer: u64) {
        self.timer.set_timer(timer);
    }

    fn increase_timer(&mut self, delta: u64) {
        self.timer.increase_timer(delta);
    }
}

impl IntChipApi for SifiveEChip {
    fn get_irq(&mut self, _: u32) -> u32 {
        self.plic.get_irq()
    }

    fn get_irq_status(
        &self,
        _: u32,
        irq: u32,
    ) -> Result<IrqStatus, UsageError> {
        self.plic.get_irq_status(irq)
    }

    fn disable_irq(
        &mut self,
        _: u32,
        irq: u32,
        also_clear_pending: bool,
    ) -> Result<(), UsageError> {
        self.plic.disable_irq(irq, also_clear_pending)
    }

    fn enable_irq(
        &mut self,
        _: u32,
        irq: u32,
        also_clear_pending: bool,
    ) -> Result<(), UsageError> {
        self.plic.enable_irq(irq, also_clear_pending)
    }
}

impl ChipApi for SifiveEChip {}

impl Default for SifiveEChip {
    fn default() -> Self {
        Self::new()
    }
}

include!(concat!(env!("OUT_DIR"), "/chip_config.rs"));
include!(concat!(env!("OUT_DIR"), "/peripherals.rs"));
