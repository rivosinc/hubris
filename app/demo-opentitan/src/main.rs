// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

#![no_std]
#![no_main]

extern crate panic_halt;
extern crate riscv_rt;

use riscv_rt::entry;

#[entry]
fn main() -> ! {
    // The opentitan requires enabling the timer and interrupts
    // for mtime and mtimecmp to behave as expected
    // See https://docs.opentitan.org/hw/ip/rv_timer/doc/
    const OT_TIMER_CTRL: u32 = 0x4010_0004;
    const OT_TIMER_INTR: u32 = 0x4010_0100;

    // Enable mtime to count up
    let timer = OT_TIMER_CTRL as *mut u32;
    unsafe{*timer |= 1;};

    // Enable interrupts when mtime passes mtimecmp
    let intr = OT_TIMER_INTR as *mut u32;
    unsafe{*intr |= 1;};

    //In RISC-V, this is just what mtimecmp gets set to.
    const MTIMECMP: u32 = 1000;
    unsafe { 
        kern::startup::start_kernel(MTIMECMP) 
    }
}
