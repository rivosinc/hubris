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
use core::ptr::NonNull;

use core::sync::atomic::Ordering;

cfg_if::cfg_if! {
    if #[cfg(riscv_no_atomics)] {
        use riscv_pseudo_atomics::atomic::AtomicBool;
    }
    else {
        use core::sync::atomic::AtomicBool;
    }
}

use zerocopy::FromBytes;

use crate::task;
use crate::time::Timestamp;
use crate::umem::USlice;
use unwrap_lite::UnwrapLite;

extern crate riscv_rt;

use riscv::register;
use riscv::register::mstatus::MPP;

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

/// On RISC-V we use a global to record the current task pointer.  It may be
/// possible to use the mscratch register instead.
#[no_mangle]
pub static mut CURRENT_TASK_PTR: Option<NonNull<task::Task>> = None;

/// To allow our clock frequency to be easily determined from a debugger, we
/// store it in memory.
#[no_mangle]
pub static mut CLOCK_FREQ_KHZ: u32 = 0;

/// RISC-V volatile registers that must be saved across context switches.
#[repr(C)]
#[derive(Clone, Debug, Default, FromBytes)]
pub struct SavedState {
    // NOTE: the following fields must be kept contiguous!
    ra: u32,
    sp: u32,
    gp: u32,
    tp: u32,
    t0: u32,
    t1: u32,
    t2: u32,
    s0: u32,
    s1: u32,
    a0: u32,
    a1: u32,
    a2: u32,
    a3: u32,
    a4: u32,
    a5: u32,
    a6: u32,
    a7: u32,
    s2: u32,
    s3: u32,
    s4: u32,
    s5: u32,
    s6: u32,
    s7: u32,
    s8: u32,
    s9: u32,
    s10: u32,
    s11: u32,
    t3: u32,
    t4: u32,
    t5: u32,
    t6: u32,
    // Additional save value for task program counter
    pc: u32,
    // NOTE: the above fields must be kept contiguous!
}

/// Map the volatile registers to (architecture-independent) syscall argument
/// and return slots.
impl task::ArchState for SavedState {
    fn stack_pointer(&self) -> u32 {
        self.sp
    }

    /// Reads syscall argument register 0.
    fn arg0(&self) -> u32 {
        self.a0
    }
    fn arg1(&self) -> u32 {
        self.a1
    }
    fn arg2(&self) -> u32 {
        self.a2
    }
    fn arg3(&self) -> u32 {
        self.a3
    }
    fn arg4(&self) -> u32 {
        self.a4
    }
    fn arg5(&self) -> u32 {
        self.a5
    }
    fn arg6(&self) -> u32 {
        self.a6
    }

    fn syscall_descriptor(&self) -> u32 {
        self.a7
    }

    /// Writes syscall return argument 0.
    fn ret0(&mut self, x: u32) {
        self.a0 = x
    }
    fn ret1(&mut self, x: u32) {
        self.a1 = x
    }
    fn ret2(&mut self, x: u32) {
        self.a2 = x
    }
    fn ret3(&mut self, x: u32) {
        self.a3 = x
    }
    fn ret4(&mut self, x: u32) {
        self.a4 = x
    }
    fn ret5(&mut self, x: u32) {
        self.a5 = x
    }
}

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

pub fn reinitialize(task: &mut task::Task) {
    *task.save_mut() = SavedState::default();

    // Set the initial stack pointer, ensuring 16-byte stack alignment as per
    // the RISC-V calling convention.
    let initial_stack = task.descriptor().initial_stack;
    task.save_mut().sp = initial_stack;
    uassert!(task.save().sp & 0xF == 0);

    // zap the stack with a distinct pattern
    for region in task.region_table().iter() {
        if initial_stack < region.base {
            continue;
        }
        if initial_stack > region.base + region.size {
            continue;
        }
        let mut uslice: USlice<u32> = USlice::from_raw(
            region.base as usize,
            (initial_stack as usize - region.base as usize) >> 2,
        )
        .unwrap_lite();

        let zap = task.try_write(&mut uslice).unwrap_lite();
        for word in zap.iter_mut() {
            *word = 0xbaddcafe;
        }
    }
    // Set the initial program counter
    task.save_mut().pc = task.descriptor().entry_point;
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

#[allow(unused_variables)]
pub fn start_first_task(tick_divisor: u32, task: &mut task::Task) -> ! {
    unsafe {
        //
        // Configure the timer
        //
        CLOCK_FREQ_KHZ = tick_divisor;

        // make MTIMECMP start with MTIME
        let mtime = core::ptr::read_volatile(crate::startup::MTIME as *mut u64);
        core::ptr::write_volatile(crate::startup::MTIMECMP as *mut u64, mtime);

        // increment mtimecmp for appropriate timer interrupts
        reset_timer();

        // Machine timer interrupt enable
        register::mie::set_mtimer();

        // Configure MPP to switch us to User mode on exit from Machine
        // mode (when we call "mret" below).
        register::mstatus::set_mpp(MPP::User);
    }

    // Write the initial task program counter.
    register::mepc::write(task.save().pc as *const usize as usize);

    // Load first task pointer, set its initial stack pointer, and exit out
    // of machine mode, launching the task.
    unsafe {
        crate::task::activate_next_task(task);
        asm!("
            lw sp, ({sp})
            mret",
            sp = in(reg) &task.save().sp,
            options(noreturn)
        );
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

/// Records the address of `task` as the current user task.
///
/// # Safety
///
/// This records a pointer that aliases `task`. As long as you don't read that
/// pointer while you have access to `task`, and as long as the `task` being
/// stored is actually in the taask table, you'll be okay.
pub unsafe fn set_current_task(task: &mut task::Task) {
    // Safety: should be ok if the contract above is met
    // TODO: make me an atomic
    unsafe {
        CURRENT_TASK_PTR = Some(NonNull::from(task));
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
