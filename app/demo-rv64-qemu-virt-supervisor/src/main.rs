// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

#![no_std]
#![no_main]

extern crate panic_halt;
extern crate riscv_rt;

use riscv_rt::entry;

#[no_mangle]
#[entry]
fn main() -> ! {
    // TODO(tdewey): This is copied over from hifive-inventor. Fix?
    const CYCLES_PER_MS: u32 = 3_200_000;

    unsafe { kern::startup::start_kernel(CYCLES_PER_MS) }
}
