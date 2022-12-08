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
use core::sync::atomic::Ordering;

cfg_if::cfg_if! {
    if #[cfg(riscv_no_atomics)] {
        use riscv_pseudo_atomics::atomic::AtomicBool;
    }
    else {
        use core::sync::atomic::AtomicBool;
    }
}

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

mod clock_freq;
pub use clock_freq::*;

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

pub fn reset() -> ! {
    unimplemented!();
}

// Constants that may change depending on configuration
include!(concat!(env!("OUT_DIR"), "/consts.rs"));
