// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use crate::arch::reset_timer;
use crate::arch::SavedState;
use crate::arch::CLOCK_FREQ_KHZ;

use crate::task;
use crate::umem::USlice;
use unwrap_lite::UnwrapLite;

use core::arch::asm;
use core::ptr::NonNull;
use riscv::register;
use riscv::register::mstatus::MPP;

/// On RISC-V we use a global to record the current task pointer.  It may be
/// possible to use the mscratch register instead.
#[no_mangle]
pub static mut CURRENT_TASK_PTR: Option<NonNull<task::Task>> = None;

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
    register::mepc::write(task.save().pc() as *const usize as usize);

    // Load first task pointer, set its initial stack pointer, and exit out
    // of machine mode, launching the task.
    unsafe {
        crate::task::activate_next_task(task);
        asm!("
            lw sp, ({sp})
            mret",
            sp = in(reg) &task.save().sp(),
            options(noreturn)
        );
    }
}

pub fn reinitialize(task: &mut task::Task) {
    *task.save_mut() = SavedState::default();

    // Set the initial stack pointer, ensuring 16-byte stack alignment as per
    // the RISC-V calling convention.
    let initial_stack = task.descriptor().initial_stack;
    task.save_mut().set_sp(initial_stack);
    uassert!(task.save().sp() & 0xF == 0);

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
    let pc = task.descriptor().entry_point;
    task.save_mut().set_pc(pc);
}
