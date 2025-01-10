// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/

use crate::arch::CLOCK_FREQ_KHZ;
use abi::{IrqStatus, UsageError};
use drv_chip_api::ChipApi;

pub static mut CHIP_API: Option<&'static mut dyn ChipApi> = None;

/// Set the reference to a chip interface
/// # Safety
/// this must be called before the kernel is started with a valid ref
pub unsafe fn set_chip_reference(chip: &'static mut dyn ChipApi) {
    unsafe {
        CHIP_API = Some(chip);
    }
}

/// Initialize the timer, should be called once
pub fn init_timer() {
    unsafe {
        let chip = CHIP_API.as_mut().expect("Uninitialized chip");
        let time = chip.get_time();
        chip.set_timer(time + CLOCK_FREQ_KHZ as u64);
    }
}

/// Increase timer for next interrupt, timer must be Initialized before
pub fn renew_timer() {
    unsafe {
        // Do not check Option to gain performance, already check once before
        let chip = CHIP_API.as_mut().unwrap_unchecked();
        chip.increase_timer(CLOCK_FREQ_KHZ as u64);
    }
}

pub fn reset() -> ! {
    // Do not check Option to gain performance, already check once before
    let chip = unsafe { CHIP_API.as_mut().unwrap_unchecked() };
    chip.reset();
}

pub fn get_external_irq(ctrl: u32) -> u32 {
    // Do not check Option to gain performance, already check once before
    let chip = unsafe { CHIP_API.as_mut().unwrap_unchecked() };
    chip.get_irq(ctrl)
}

pub fn get_external_irq_status(
    src: &abi::InterruptSource,
) -> Result<IrqStatus, UsageError> {
    unsafe {
        // Do not check Option to gain performance, already check once before
        let chip = CHIP_API.as_ref().unwrap_unchecked();
        chip.get_irq_status(src.controller, src.irq)
    }
}

pub fn disable_external_irq(
    src: &abi::InterruptSource,
    also_clear_pending: bool,
) -> Result<(), UsageError> {
    unsafe {
        // Do not check Option to gain performance, already check once before
        let chip = CHIP_API.as_mut().unwrap_unchecked();
        chip.disable_irq(src.controller, src.irq, also_clear_pending)
    }
}

pub fn enable_external_irq(
    src: &abi::InterruptSource,
    also_clear_pending: bool,
) -> Result<(), UsageError> {
    unsafe {
        // Do not check Option to gain performance, already check once before
        let chip = CHIP_API.as_mut().unwrap_unchecked();
        chip.enable_irq(src.controller, src.irq, also_clear_pending)
    }
}
