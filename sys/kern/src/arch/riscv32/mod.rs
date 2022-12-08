// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Architecture support for RISC-V.
//!
//! The kernel should support any riscv32imc and riscv32imac target.
//! There is no Supervisor mode support; the kernel runs exclusively in Machine
//! mode with tasks running in User mode.
//!
//! Interrupts are supported through the PLIC, but due to the nature of their
//! implementation here it's not possible for the kernel to support core
//! interrupts on the lines reserved for custom extensions. To fix this,
//! the external interrupt controller will need to be treated like an external
//! device, and have a driver task.

use core::arch::asm;
#[cfg(feature = "custom-interrupts")]
use core::convert::TryInto;

use core::sync::atomic::Ordering;

cfg_if::cfg_if! {
    if #[cfg(riscv_no_atomics)] {
        use riscv_pseudo_atomics::atomic::AtomicBool;
    }
    else {
        use core::sync::atomic::AtomicBool;
    }
}

use crate::time::Timestamp;

extern crate riscv_rt;

#[allow(unused)]
macro_rules! uassert {
    ($cond : expr) => {
        if !$cond {
            panic!("Assertion failed!");
        }
    };
}

mod pmp;
pub use pmp::*;

mod mtimer;
pub use mtimer::*;

mod trap;
pub use trap::*;

mod saved_state;
pub use saved_state::*;

mod task;
pub use task::*;

/// To allow our clock frequency to be easily determined from a debugger, we
/// store it in memory.
#[no_mangle]
pub static mut CLOCK_FREQ_KHZ: u32 = 0;

// Because debuggers need to know the clock frequency to set the SWO clock
// scaler that enables ITM, and because ITM is particularly useful when
// debugging boot failures, this should be set as early in boot as it can
// be.
pub fn set_clock_freq(tick_divisor: u32) {
    // TODO switch me to an atomic. Note that this may break Humility.
    // SAFETY:
    // In a single-threaded, single-process context (which the kernel is in),
    // access to global mutables are safe as data races are impossible.
    unsafe {
        CLOCK_FREQ_KHZ = tick_divisor;
    }
}

cfg_if::cfg_if! {
    if #[cfg(feature = "vectored-interrupts")] {
        use riscv::register::mtvec::{self, TrapMode};

        // Setup interrupt vector `mtvec` with vectored mode to the trap table.
        #[export_name = "_setup_interrupts"]
        extern "C" fn _setup_interrputs() {
            // SAFETY:
            // If `_trap_table` does not have the neccasary alignment, the
            // address could become corrupt and traps will not jump to the
            // expected address. As long as the linker works correctly, this
            // write is safe.
            unsafe { mtvec::write(_trap_table as usize, TrapMode::Vectored); };
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
        /// # Safety
        /// All of the entries jump to the same trap routine, so as long as they
        /// don't get corrupted this should always go to `_start_trap`.
        /// This table being corrupted will lead to undefined behavior.
        unsafe extern "C" fn _trap_table() {
            unsafe { asm!( "
                .rept 256 # TODO: This may need to be changed
                j _start_trap
                .endr
                ",
                options(noreturn),
            );}
        }
    }
}

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

#[used]
pub static mut TICKS: u64 = 0;

/// Reads the tick counter.
pub fn now() -> Timestamp {
    Timestamp::from(unsafe { TICKS })
}

pub fn reset() -> ! {
    unimplemented!();
}

// Constants that may change depending on configuration
include!(concat!(env!("OUT_DIR"), "/consts.rs"));
