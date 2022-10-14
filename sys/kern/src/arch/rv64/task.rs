// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use crate::arch::set_timer;
use crate::arch::SavedState;
use crate::task;
use crate::umem::USlice;

use core::arch::asm;
use riscv::register;
#[cfg(feature = "riscv-supervisor-mode")]
use riscv::register::{
    scause as xcause, scause::Exception as xcauseException,
    scause::Interrupt as xcauseInterrupt,
    scause::Interrupt::SupervisorTimer as xInterruptTimer,
    scause::Trap as xcauseTrap, sepc as xepc, sie as xie,
    sie::set_stimer as set_xtimer, sscratch as xscratch, sstatus as xstatus,
    sstatus::set_spp as set_xpp, sstatus::SPP as XPP, stval as xtval,
    stvec as xtvec, stvec::TrapMode as xTrapMode,
};

#[cfg(not(feature = "riscv-supervisor-mode"))]
use riscv::register::{
    mcause as xcause, mcause::Exception as xcauseException,
    mcause::Interrupt as xcauseInterrupt,
    mcause::Interrupt::SupervisorTimer as xInterruptTimer,
    mcause::Trap as xcauseTrap, mepc as xepc, mie as xie,
    mie::set_mtimer as set_xtimer, mscratch as xscratch, mstatus as xstatus,
    mstatus::set_mpp as set_xpp, mstatus::MPP as XPP, mtval as xtval,
    mtvec as xtvec, mtvec::TrapMode as xTrapMode,
};

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
    let task = task as *const task::Task as usize;

    xscratch::write(task);
}

pub unsafe fn get_current_task() -> &'static task::Task {
    let task = xscratch::read();
    uassert!(task != 0);
    unsafe { &*(task as *const task::Task) }
}

#[allow(unused_variables)]
pub fn start_first_task(tick_divisor: u32, task: &mut task::Task) -> ! {
    // Configure MPP to switch us to User mode on exit from Machine
    // mode (when we call "mret" below).
    unsafe {
        set_xpp(XPP::User);
    }

    // Write the initial task program counter.
    xepc::write(task.save().pc() as *const usize as usize);

    //
    // Configure the timer
    //
    unsafe {
        // Reset mtime back to 0, set mtimecmp to chosen timer
        set_timer(tick_divisor - 1);

        // Machine timer interrupt enable
        set_xtimer();
    }

    // Load first task pointer, set its initial stack pointer, and exit out
    // of machine mode, launching the task.
    unsafe {
        crate::task::activate_next_task(task);
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
