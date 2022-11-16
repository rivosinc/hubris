// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Client API for Reset

#![no_std]

use derive_idol_err::IdolError;
use serde::{Deserialize, Serialize};
use userlib::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(C)]
pub enum ResetType {
    Shutdown,
    ColdReboot,
    WarmReboot,
}

/// Platform-agnostic (but heavily influenced) reset status bits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(C)]
pub enum ResetReason {
    PowerOn,
    Pin,
    SystemCall,
    Brownout,
    SystemWatchdog,
    IndependentWatchdog,
    LowPowerSecurity,
    ExitStandby,
    Other(u32),
    Unknown, // TODO remove and use `Option<ResetReason>` once we switch to hubpack
}

#[derive(Copy, Clone, Debug, FromPrimitive, Eq, PartialEq, IdolError)]
#[repr(u32)]
pub enum ResetError {
    NotImplemented = 1,
}

include!(concat!(env!("OUT_DIR"), "/client_stub.rs"));
