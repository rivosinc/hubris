// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

#![no_std]

pub use abi::{IrqStatus, UsageError};

pub trait IntCtrlApi {
    /// Return the interrupt number that has triggered
    fn get_irq(&mut self) -> u32;

    /// Return the status of an interrupt on an interrupt controller
    fn get_irq_status(&self, irq: u32) -> Result<IrqStatus, UsageError>;

    /// disable an interrupt
    fn disable_irq(
        &mut self,
        irq: u32,
        also_clear_pending: bool,
    ) -> Result<(), UsageError>;

    /// enable an interrupt
    fn enable_irq(
        &mut self,
        irq: u32,
        also_clear_pending: bool,
    ) -> Result<(), UsageError>;
}
