// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

#![no_std]
#![no_main]

use userlib::*;

#[export_name = "main"]
fn main() -> ! {
    sys_log!("Hello world from one-shot task!");
    sys_log!("Exiting now!");
    kipc::exit_current_task();

    // Adding this just to make rustc happy
    loop {}
}
