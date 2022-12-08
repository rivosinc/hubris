// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use core::sync::atomic::Ordering;

cfg_if::cfg_if! {
    if #[cfg(riscv_no_atomics)] {
        use riscv_pseudo_atomics::atomic::AtomicBool;
    }
    else {
        use core::sync::atomic::AtomicBool;
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
