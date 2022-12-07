// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/

use crate::arch::CLOCK_FREQ_KHZ;

// Timer handling.
//
// We currently only support single HART systems.  From reading elsewhere,
// additional harts have their own mtimecmp offset at 0x8 intervals from hart0.
//
// Configure the timer.
//
// RISC-V Privileged Architecture Manual
// 3.2.1 Machine Timer Registers (mtime and mtimecmp)
//
pub fn reset_timer() {
    //
    // Increase mtimecmp for the next interrupt
    // This will also clear the pending timer interrupt.
    //
    unsafe {
        let mut mtimecmp =
            core::ptr::read_volatile(crate::startup::MTIMECMP as *mut u64);
        mtimecmp += CLOCK_FREQ_KHZ as u64;
        core::ptr::write_volatile(
            crate::startup::MTIMECMP as *mut u64,
            mtimecmp,
        );
    }
}
