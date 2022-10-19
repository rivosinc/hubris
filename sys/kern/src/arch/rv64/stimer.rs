// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.


// Configure the timer.
//
// RISC-V Privileged Architecture Manual
// 3.2.1 Machine Timer Registers (mtime and mtimecmp)
//
// To keep things simple, especially on RV32 systems where we cannot atomically
// write to the mtime/mtimecmp memory-mapped registers as they are 64 bits
// wide, we only utilise the first 32-bits of each register, setting the
// high-order bits to 0 on startup, and restarting the low-order bits of mtime
// back to 0 on each interrupt.
//
#[no_mangle]
pub unsafe fn set_timer(tick_divisor: u32) {
    // TODO: SBI Call
}

pub fn reset_timer() {
    // TODO: SBI Call
}
