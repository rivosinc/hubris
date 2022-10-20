// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use core::arch::asm;

// Module-internal fns
fn sbicall(eid: usize, fid: usize) -> (usize, usize) {
    let value: usize;
    let error: usize;

    unsafe {
        asm!("ecall",
            in("a6") fid, in("a7") eid,
            out("a0") error, out("a1") value
        );
    }
    (error, value)
}

fn sbicall1(eid: usize, fid: usize, arg0: usize) -> (usize, usize) {
    let value: usize;
    let mut a0 = arg0;

    unsafe {
        asm!("ecall",
            inout("a0") a0, in("a6") fid, in("a7") eid,
            out("a1") value
        );
    }
    (a0, value)
}

fn sbicall2(
    eid: usize,
    fid: usize,
    arg0: usize,
    arg1: usize,
) -> (usize, usize) {
    let mut a0 = arg0;
    let mut a1 = arg1;

    unsafe {
        asm!("ecall",
            inout("a0") a0, inout("a1") a1, in("a6") fid, in("a7") eid
        );
    }
    (a0, a1)
}

// RISC-V SBI Specification 1.0
// Chapter 0, Base EID
const SBI_EID_BASE: usize = 0x10;
const SBI_FID_BASE_GET_SPEC_VERSION: usize = 0x0;
const SBI_FID_BASE_GET_SBI_VERSION: usize = 0x02;

pub fn sbi_get_spec_version() -> (usize, usize) {
    sbicall(SBI_EID_BASE, SBI_FID_BASE_GET_SPEC_VERSION)
}

pub fn sbi_get_sbi_version() -> (usize, usize) {
    sbicall(SBI_EID_BASE, SBI_FID_BASE_GET_SBI_VERSION)
}

// Chapter 6, Timer Extension EID
/// "TIME"
const SBI_EID_TIMER: usize = 0x54494D45;
const SBI_FID_TIMER_SET_TIMER: usize = 0x0;

pub fn sbi_set_timer(stime_value: u64) -> (usize, usize) {
    sbicall1(SBI_EID_TIMER, SBI_FID_TIMER_SET_TIMER, stime_value as usize)
}

// Chapter 10, System Reset Extension EID
/// "SRST"
const SBI_EID_SYSTEM_RESET: usize = 0x53525354;
const SBI_FID_SYSTEM_RESET_SYSTEM_RESET: usize = 0x0;
pub enum ResetType {
    Shutdown = 0x0,
    ColdReboot = 0x1,
    WarmReboot = 0x2,
}

pub enum ResetReason {
    NoReason = 0x0,
    SystemFailure = 0x1,
}

pub fn sbi_system_reset(
    reset_type: ResetType,
    reset_reason: ResetReason,
) -> (usize, usize) {
    sbicall2(
        SBI_EID_SYSTEM_RESET,
        SBI_FID_SYSTEM_RESET_SYSTEM_RESET,
        reset_type as usize,
        reset_reason as usize,
    )
}
