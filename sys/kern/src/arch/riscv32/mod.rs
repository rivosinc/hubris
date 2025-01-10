// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Architecture support for RISC-V.
//!
//! The kernel should support any riscv32imc and riscv32imac target.
//! There is no Supervisor mode support; the kernel runs exclusively in Machine
//! mode with tasks running in User mode.
#[allow(unused)]
macro_rules! uassert {
    ($cond : expr) => {
        if !$cond {
            panic!("Assertion failed!");
        }
    };
}

mod chip;
pub use chip::*;

mod pmp;
pub use pmp::*;

mod trap;
pub use trap::*;

mod saved_state;
pub use saved_state::*;

mod task;
pub use task::*;

mod clock_freq;
pub use clock_freq::*;

mod atomics;
