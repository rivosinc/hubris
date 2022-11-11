// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use core::sync::atomic::{AtomicBool, Ordering};

#[cfg(feature = "riscv-supervisor-mode")]
use crate::arch::{sbi_system_reset, ResetReason, ResetType};

impl crate::atomic::AtomicExt for AtomicBool {
    type Primitive = bool;

    #[inline(always)]
    fn swap_polyfill(
        &self,
        value: Self::Primitive,
        ordering: Ordering,
    ) -> Self::Primitive {
        self.swap(value, ordering)
    }
}

// Use QEMU's memory-mapped sifive_test device to attempt
// restart/poweroff.
#[cfg(feature = "riscv-sifive_test-device")]
mod sifive_test {
    pub const RESET_ADDR: *mut u32 = 0x00100000 as *mut u32;
    pub const REBOOT_VALUE: u32 = 0x00007777;
    pub const _POWEROFF_VALUE: u32 = 0x00005555;
    pub const _FAIL_VALUE: u32 = 0x00003333;
}

#[cfg(feature = "riscv-supervisor-mode")]
pub fn reset() -> ! {
    sbi_system_reset(ResetType::ColdReboot, ResetReason::NoReason);
    unreachable!();
}

#[cfg(feature = "riscv-sifive_test-device")]
pub fn reset() -> ! {
    unsafe {
        *sifive_test::RESET_ADDR = sifive_test::REBOOT_VALUE;
    }
    unreachable!();
}

#[cfg(not(any(
    feature = "riscv-supervisor-mode",
    feature = "riscv-sifive_test-device"
)))]
pub fn reset() -> ! {
    unimplemented!();
}
