// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

#![no_std]
#![no_main]

use userlib::*;

use task_reset_api::{ResetReason, ResetType};

task_slot!(RESET, reset_driver);

#[export_name = "main"]
fn main() -> ! {
    let reset = task_reset_api::Reset::from(crate::RESET.get_task_id());
    reset
        .reset(ResetType::ColdReboot, ResetReason::SystemCall)
        .unwrap();
    unreachable!();
}
