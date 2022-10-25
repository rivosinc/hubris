// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use crate::arch::sbi_set_timer;
use crate::arch::CLOCK_FREQ_KHZ;

use core::convert::TryInto;

#[cfg(feature = "riscv-supervisor-mode")]
const HAS_SSTC: bool = true;

#[cfg(not(feature = "riscv-supervisor-mode"))]
const HAS_SSTC: bool = false;

#[no_mangle]
pub unsafe fn set_timer(tick_divisor: usize) {
    // TODO: Feature detection for Sstc, SBI call for non-supporting, supporting get:
    //       riscv::register::stimecmp::write(tick_divisor.into());
    if HAS_SSTC {
        riscv::register::stimecmp::write(tick_divisor as usize)
    } else {
        sbi_set_timer(tick_divisor.try_into().unwrap());
    }
}

pub fn reset_timer() {
    unsafe {
        if HAS_SSTC {
            let time = riscv::register::stimecmp::read() + CLOCK_FREQ_KHZ as usize;
            riscv::register::stimecmp::write(time)
        } else {
            let time = riscv::register::time::read() + CLOCK_FREQ_KHZ as usize;
            sbi_set_timer(time.try_into().unwrap());
        }
    }
}
