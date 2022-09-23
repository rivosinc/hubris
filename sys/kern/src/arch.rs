// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Architecture-specific support.
//!
//! In practice, this works by
//!
//! - Conditionally defining a nested module (below).
//! - `pub use`-ing its contents
//!
//! Thus, all architecture-specific types and functions show up right here in
//! the `arch` module, magically tailored for the current target.
//!
//! For this to work, each architecture support module must define the same set
//! of names.

cfg_if::cfg_if! {
    // Note: cfg_if! is slightly touchy about ordering and expression
    // complexity; this chain seems to be the best compromise.

    if #[cfg(target_arch = "arm")] {
        #[macro_use]
        pub mod arm;
        pub use arm::*;
    } else if #[cfg(target_arch = "riscv32")] {
        #[macro_use]
        pub mod rv32;
        pub use rv32::*;
    } else if #[cfg(target_arch = "riscv64")] {
        #[macro_use]
        pub mod rv64;
        pub use rv64::*;
    } else {
        compile_error!("support for this architecture not implemented");
    }
}
