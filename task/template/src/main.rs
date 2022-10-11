// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

#![no_std]
#![no_main]

use userlib::*;

#[export_name = "main"]
fn main() -> ! {
    loop {
        // NOTE: you need to put code here before running this! Otherwise LLVM
        // will turn this into a single undefined instruction.
    }
}

// This line includes the config file generated in the `build.rs`. Currently,
// it will add constants for any peripherals and interrupts used. See
// the `riscv-plic-server` and `fe310-rtc` drivers for examples on how these
// are used.
//
// This can be removed if the task has no peripherals and no interrupts to
// handle.
include!(concat!(env!("OUT_DIR"), "/config.rs"));
