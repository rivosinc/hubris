// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use crate::arch::sbi_set_timer;

#[no_mangle]
pub unsafe fn set_timer(tick_divisor: u32) {
    // TODO: Feature detection for Sstc, SBI call for non-supporting, supporting get:
    //       riscv::register::stime::write(0);
    //       riscv::register::stimecmp::write(tick_divisor.into());
    sbi_set_timer(tick_divisor.into());
}

pub fn reset_timer() {
    // TODO: Feature detection for Sstc, SBI call for non-supporting, supporting get:
    //       riscv::register::stime::write(0);
    sbi_set_timer(0);
}
