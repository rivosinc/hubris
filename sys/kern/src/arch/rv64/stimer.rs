// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use crate::arch::clock_freq::CLOCK_FREQ_KHZ;
use crate::arch::sbi_set_timer;

#[no_mangle]
pub unsafe fn set_timer() {
    let current = riscv::register::time::read();

    if cfg!(feature = "riscv-support-sstc") {
        riscv::register::stimecmp::write(current)
    } else {
        sbi_set_timer(current as u64);
    }
}

pub fn reset_timer() {
    let current = riscv::register::time::read();

    // Safety: CLOCK_FREQ_KHZ is a public static mutable, but is only
    //         ever set at start of day.
    unsafe {
        let destination = current as u64 + CLOCK_FREQ_KHZ as u64;
        if cfg!(feature = "riscv-support-sstc") {
            riscv::register::stimecmp::write(destination as usize)
        } else {
            sbi_set_timer(destination);
        }
    }
}
