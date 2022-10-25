// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use core::arch::asm;

#[cfg(feature = "riscv-supervisor-mode")]
use riscv::register::stvec as xtvec;

#[cfg(not(feature = "riscv-supervisor-mode"))]
use riscv::register::mtvec as xtvec;

// Setup interrupt vector `xtvec` with vectored mode to the trap table.
// SAFETY: if _start_trap does not have the neccasary alignment,
// the address could become corrupt and traps will not jump to the
// expected address
#[export_name = "_setup_interrupts"]
pub unsafe extern "C" fn _setup_interrupts() {
    unsafe {
        xtvec::write(_trap_table as usize, xtvec::TrapMode::Vectored);
    };
}

// Create a trap table to vector interrupts to the correct handler.
// NOTE: This MUST be aligned to at least a 4-byte boundary. Some
//       targets have larger requirements, so we've gone with the
//       highest so far: 256.
// TODO: Currently all pass through common function, but can be vectored
//       directly
#[naked]
#[no_mangle]
#[repr(align(0x100))]
#[link_section = ".trap.rust"]
#[export_name = "_trap_table"]
pub unsafe extern "C" fn _trap_table() {
    unsafe {
        asm!(
            "
        .rept 256 # TODO: This may need to be changed
        j _start_trap
        .endr
        ",
            options(noreturn),
        );
    }
}
