// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use crate::arch::sbi_set_timer;

const HAS_SSTC: bool = false; // TODO: Runtime detection

#[no_mangle]
pub unsafe fn set_timer(tick_divisor: u32) {
    // TODO: Feature detection for Sstc, SBI call for non-supporting, supporting get:
    //       riscv::register::stimecmp::write(tick_divisor.into());
    if HAS_SSTC {
        riscv::register::stimecmp::write(tick_divisor as usize)
    } else {
        sbi_set_timer(tick_divisor.into());
    }
}

pub fn reset_timer() {
    if HAS_SSTC {
        riscv::register::stimecmp::write(0)
    } else {
        sbi_set_timer(0);
    }
}
