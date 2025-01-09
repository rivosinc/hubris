// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

#![no_std]

pub use abi::{IrqStatus, UsageError};

pub trait IntChipApi {
    /// Return the interrupt number that has triggered
    /// on an interrupt controller
    fn get_irq(&mut self, ctrl: u32) -> u32;

    /// Return the status of an interrupt on an interrupt controller
    fn get_irq_status(
        &self,
        ctrl: u32,
        irq: u32,
    ) -> Result<IrqStatus, UsageError>;

    /// disable an interrupt on an interrupt controller
    fn disable_irq(
        &mut self,
        ctrl: u32,
        irq: u32,
        also_clear_pending: bool,
    ) -> Result<(), UsageError>;

    /// enable an interrupt on an interrupt controller
    fn enable_irq(
        &mut self,
        ctrl: u32,
        irq: u32,
        also_clear_pending: bool,
    ) -> Result<(), UsageError>;
}
