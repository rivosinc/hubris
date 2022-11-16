// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Driver for default reset.
//!
//! Use the reset-api crate to interact with this driver.

#![no_std]
#![no_main]

use idol_runtime::RequestError;
use task_reset_api::ResetError;

use userlib::kipc;

struct ResetServer();

impl idl::InOrderResetImpl for ResetServer {
    fn reset(
        &mut self,
        _: &userlib::RecvMessage,
        _type: task_reset_api::ResetType,
        _reason: task_reset_api::ResetReason,
    ) -> Result<(), RequestError<ResetError>> {
        kipc::system_restart();
    }
}

#[export_name = "main"]
fn main() -> ! {
    let mut reset = ResetServer();
    let mut buffer = [0u8; idl::INCOMING_SIZE];

    loop {
        idol_runtime::dispatch(&mut buffer, &mut reset);
    }
}

mod idl {
    use task_reset_api::{ResetError, ResetReason, ResetType};

    include!(concat!(env!("OUT_DIR"), "/server_stub.rs"));
}
