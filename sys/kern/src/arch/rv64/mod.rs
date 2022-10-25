// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

#[macro_use]
pub mod macros;
pub use macros::*;

#[macro_use]
pub mod trap;
pub use trap::*;

pub mod task;
pub use task::*;

pub mod saved_state;
pub use saved_state::*;

pub mod clock_freq;
pub use clock_freq::*;

cfg_if::cfg_if! {
    if #[cfg(feature = "vectored-interrupts")] {
        pub mod interrupts;
        pub use interrupts::*;
    }
}

cfg_if::cfg_if! {
    if #[cfg(feature = "riscv-supervisor-mode")] {
        pub mod stimer;
        pub use stimer::*;

        pub mod mmu;
        pub use mmu::*;

        pub mod sbi;
        pub use sbi::*;
    } else {
        pub mod mtimer;
        pub use mtimer::*;

        pub mod pmp;
        pub use pmp::*;
    }
}

pub mod ticks;
pub use ticks::*;

pub mod helpers;
pub use helpers::*;
