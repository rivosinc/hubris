// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

macro_rules! uassert {
    ($cond : expr) => {
        if !$cond {
            panic!("Assertion failed!");
        }
    };
}

cfg_if::cfg_if! {
    if #[cfg(feature = "klog-semihosting")] {
        macro_rules! klog {
            ($s:expr) => { let _ = riscv_semihosting::hprintln!($s); };
            ($s:expr, $($tt:tt)*) => { let _ = riscv_semihosting::hprintln!($s, $($tt)*); };
        }
    } else {
        macro_rules! klog {
            ($s:expr) => { };
            ($s:expr, $($tt:tt)*) => { };
        }
    }
}
