// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Driver for sifive_test reset.
//!
//! Use the reset-api crate to interact with this driver.

#![no_std]
#![no_main]

use idol_runtime::RequestError;
use task_reset_api::ResetError;
use task_reset_api::ResetType::*;

const RESET_ADDR: *mut u32 = 0x00100000 as *mut u32;
const REBOOT_VALUE: u32 = 0x00007777;
const POWEROFF_VALUE: u32 = 0x00005555;
const _FAIL_VALUE: u32 = 0x00003333;

struct ResetServer {
    sifive_test: *mut u32,
}

impl idl::InOrderResetImpl for ResetServer {
    fn reset(
        &mut self,
        _: &userlib::RecvMessage,
        reset_type: task_reset_api::ResetType,
        _reset_reason: task_reset_api::ResetReason,
    ) -> Result<(), RequestError<ResetError>> {
        unsafe {
            self.sifive_test.write_volatile(match reset_type {
                Shutdown => POWEROFF_VALUE,
                ColdReboot | WarmReboot => REBOOT_VALUE,
            });
        }
        unreachable!();
    }
}

#[export_name = "main"]
fn main() -> ! {
    let mut reset = ResetServer {
        sifive_test: RESET_ADDR,
    };
    let mut buffer = [0u8; idl::INCOMING_SIZE];

    loop {
        idol_runtime::dispatch(&mut buffer, &mut reset);
    }
}

mod idl {
    use task_reset_api::{ResetError, ResetReason, ResetType};

    include!(concat!(env!("OUT_DIR"), "/server_stub.rs"));
}
