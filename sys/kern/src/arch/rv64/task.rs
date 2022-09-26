// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use crate::arch::set_timer;
use crate::arch::SavedState;
use crate::task;
use crate::umem::USlice;
use core::arch::asm;
use riscv::register;
use unwrap_lite::UnwrapLite;

/// Records the address of `task` as the current user task in mscratch.
///
/// # Safety
///
/// This records a pointer that aliases `task`. As long as you don't read that
/// pointer while you have access to `task`, and as long as the `task` being
/// stored is actually in the task table, you'll be okay.
pub unsafe fn set_current_task(task: &task::Task) {
    // Safety: should be ok if the contract above is met
    // TODO: make me an atomic
    unsafe {
        let task: usize = core::mem::transmute::<&task::Task, usize>(task);
        asm!(
            "csrw mscratch, {0}",
            in(reg) task
        );
    }
}

pub unsafe fn get_current_task() -> &'static task::Task {
    let mut task: usize;
    unsafe {
        asm!("csrr {0}, mscratch", out(reg) task);
        uassert!(task != 0);
        core::mem::transmute::<usize, &task::Task>(task)
    }
}

#[allow(unused_variables)]
pub fn start_first_task(tick_divisor: u32, task: &task::Task) -> ! {
    // Configure MPP to switch us to User mode on exit from Machine
    // mode (when we call "mret" below).
    unsafe {
        use riscv::register::mstatus::{set_mpp, MPP};
        set_mpp(MPP::User);
    }

    // Write the initial task program counter.
    register::mepc::write(task.save().pc() as *const usize as usize);

    //
    // Configure the timer
    //
    unsafe {
        // Reset mtime back to 0, set mtimecmp to chosen timer
        set_timer(tick_divisor - 1);

        // Machine timer interrupt enable
        register::mie::set_mtimer();
    }

    // Load first task pointer, set its initial stack pointer, and exit out
    // of machine mode, launching the task.
    unsafe {
        set_current_task(task);
        asm!("
            ld sp, ({0})
            mret",
            in(reg) &task.save().sp(),
            options(noreturn)
        );
    }
}

pub fn reinitialize(task: &mut task::Task) {
    *task.save_mut() = SavedState::default();

    // Set the initial stack pointer, ensuring 16-byte stack alignment as per
    // the RISC-V callineg convention.
    let initial_stack: usize = task.descriptor().initial_stack;
    task.save_mut().set_sp(initial_stack as u64);
    uassert!(task.save().sp() & 0xF == 0);

    // zap the stack with a distinct pattern
    for region in task.region_table().iter() {
        if initial_stack < region.base {
            continue;
        }
        if initial_stack > region.base + region.size {
            continue;
        }

        let mut uslice: USlice<usize> = USlice::from_raw(
            region.base as usize,
            (initial_stack as usize - region.base as usize) >> 4,
        )
        .unwrap_lite();

        let zap = task.try_write(&mut uslice).unwrap_lite();
        for word in zap.iter_mut() {
            *word = 0xbaddcafebaddcafe;
        }
    }
    // Set the initial program counter
    let pc = task.descriptor().entry_point as u64;
    task.save_mut().set_pc(pc);
}
