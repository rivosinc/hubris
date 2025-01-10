// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Provides fake atomic read-modify-write operations for situations where you
//! _really_ know what you're doing.
//!
//! Pulling this trait in can cause code written for ARMv7-M and later machines,
//! which have atomic read-modify-write operations, to compile on ARMv6-M. This
//! is, in general, not safe: the program wanted an atomic read-modify-write and
//! you're faking it with a non-atomic sequence. However, in our _specific_ case
//! on Hubris, we can do this safely because
//!
//! 1. Tasks are isolated.
//! 2. Tasks are single-threaded.
//! 3. ISRs do not access memory shared with tasks.
//!
//! If any of those three points is wrong in your case, do not use these, it
//! will go badly for you.
//!
//! Everything in this crate is conditional on the `armv6m` config, which means
//! (1) it will only work inside the Hubris build system and (2) accidentally
//! including it on armv7m or later won't pull in the bogus implementations.
//!
//! # Why not just disable interrupts
//!
//! Because (1) we can't from unprivileged mode and (2) we don't out of
//! principle anyway.

#![no_std]

#[macro_export]
macro_rules! use_atomic_bool_ext {
    () => {
        #[cfg(any(armv6m, riscv_no_atomics))]
        use $crate::AtomicBoolExt;
    };
}

#[macro_export]
macro_rules! use_atomic_u32_ext {
    () => {
        #[cfg(any(armv6m, riscv_no_atomics))]
        use $crate::AtomicU32Ext;
    };
}

use core::sync::atomic::Ordering;

cfg_if::cfg_if! {
    if #[cfg(riscv_no_atomics)] {
        // Here we assume that RISC-V CPU without atomic support are single
        // core CPUs. It is fine to not have atomic as the RISC-V
        // implementation does not enable nested interrupts, we don't have
        // any case of pre-emption either in the kernel nor in the tasks.
        // (we do have task pre-emption but the task will resume at the same
        // point with no modification to its context)
        use core::cell::UnsafeCell;

        #[repr(C, align(1))]
        pub struct AtomicBool {
            v: UnsafeCell<u8>
        }

        impl Default for AtomicBool {
            /// Creates an `AtomicBool` initialized to `false`.
            #[inline]
            fn default() -> Self {
                Self::new(false)
            }
        }

        unsafe impl Sync for AtomicBool {}

        #[cfg_attr(target_pointer_width = "16", repr(C, align(2)))]
        #[cfg_attr(target_pointer_width = "32", repr(C, align(4)))]
        #[cfg_attr(target_pointer_width = "64", repr(C, align(8)))]
        pub struct AtomicPtr<T> {
            p: UnsafeCell<*mut T>,
        }

        unsafe impl<T> Sync for AtomicPtr<T> {}

        #[repr(C, align(4))]
        pub struct AtomicU32 {
            v: UnsafeCell<u32>
        }

        unsafe impl Sync for AtomicU32 {}

        impl AtomicBool {
            #[inline]
            #[must_use]
            pub const fn new(v: bool) -> AtomicBool {
                AtomicBool {
                    v: UnsafeCell::new(v as u8),
                }
            }

            #[inline]
            pub fn load(&self, _order: Ordering) -> bool {
                unsafe {
                    let ptr = self.v.get();
                    *ptr != 0
                }
            }

            #[inline]
            pub fn store(&self, val: bool, _order: Ordering) {
                unsafe {
                    let ptr = self.v.get();
                    *ptr = val as u8;
                }
            }
        }

        impl<T> AtomicPtr<T> {
            #[inline]
            pub const fn new(p: *mut T) -> AtomicPtr<T> {
                AtomicPtr {
                    p: UnsafeCell::new(p),
                }
            }

            #[inline]
            pub fn load(&self, _order: Ordering) -> *mut T {
                unsafe {
                    let ptr = self.p.get();
                    *ptr
                }
            }

            #[inline]
            pub fn store(&self, ptr: *mut T, _order: Ordering) {
                unsafe {
                    let p = self.p.get();
                    *p = ptr;
                }
            }
        }

        impl AtomicU32 {
            pub const fn new(v: u32) -> Self {
                Self {v: UnsafeCell::new(v)}
            }

            #[inline]
            pub fn load(&self, _order: Ordering) -> u32 {
                unsafe {
                    let ptr = self.v.get();
                    *ptr
                }
            }

            #[inline]
            pub fn store(&self, val: u32, _order: Ordering) {
                unsafe {
                    let ptr = self.v.get();
                    *ptr = val;
                }
            }
        }
    } else {
        pub use core::sync::atomic::{AtomicBool, AtomicPtr, AtomicU32};
    }
}

pub trait AtomicU32Ext {
    fn swap(&self, val: u32, order: Ordering) -> u32;
    fn fetch_add(&self, val: u32, order: Ordering) -> u32;
    fn fetch_sub(&self, val: u32, order: Ordering) -> u32;
}

cfg_if::cfg_if! {
    if #[cfg(any(armv6m, riscv_no_atomics))] {
        impl AtomicU32Ext for AtomicU32 {
            #[inline]
            fn swap(&self, val: u32, order: Ordering) -> u32 {
                let (lo, so) = rmw_ordering(order);
                let rv = self.load(lo);
                self.store(val, so);
                rv
            }

            #[inline]
            fn fetch_add(&self, val: u32, order: Ordering) -> u32 {
                let (lo, so) = rmw_ordering(order);
                let rv = self.load(lo);
                self.store(rv.wrapping_add(val), so);
                rv
            }

            #[inline]
            fn fetch_sub(&self, val: u32, order: Ordering) -> u32 {
                let (lo, so) = rmw_ordering(order);
                let rv = self.load(lo);
                self.store(rv.wrapping_sub(val), so);
                rv
            }
        }
    } else {
        impl AtomicU32Ext for AtomicU32 {
            #[inline]
            fn swap(&self, val: u32, order: Ordering) -> u32 {
                core::sync::atomic::AtomicU32::swap(self, val, order)
            }

            #[inline]
            fn fetch_add(&self, val: u32, order: Ordering) -> u32 {
                core::sync::atomic::AtomicU32::fetch_add(self, val, order)
            }

            #[inline]
            fn fetch_sub(&self, val: u32, order: Ordering) -> u32 {
                core::sync::atomic::AtomicU32::fetch_sub(self, val, order)
            }
        }
    }
}

pub trait AtomicBoolExt {
    fn swap(&self, val: bool, order: Ordering) -> bool;
}

cfg_if::cfg_if! {
    if #[cfg(any(armv6m, riscv_no_atomics))] {
        impl AtomicBoolExt for AtomicBool {
            #[inline]
            fn swap(&self, new: bool, order: Ordering) -> bool {
                let (lo, so) = rmw_ordering(order);
                let orig = self.load(lo);
                self.store(new, so);
                orig
            }
        }
    } else {
        impl AtomicBoolExt for AtomicBool {
            #[inline]
            fn swap(&self, new: bool, order: Ordering) -> bool {
                core::sync::atomic::AtomicBool::swap(self, new, order)
            }
        }
    }
}

#[cfg(any(armv6m, riscv_no_atomics))]
fn rmw_ordering(o: Ordering) -> (Ordering, Ordering) {
    match o {
        Ordering::AcqRel => (Ordering::Acquire, Ordering::Release),
        Ordering::Relaxed => (o, o),
        Ordering::SeqCst => (o, o),
        Ordering::Acquire => (Ordering::Acquire, Ordering::Relaxed),
        Ordering::Release => (Ordering::Relaxed, Ordering::Release),
        _ => panic!(),
    }
}
