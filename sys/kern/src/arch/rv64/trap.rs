// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Interrupts (other than the Machine Timer used to advance the kernel
//! timestamp) are not yet supported.

use core::arch::asm;

use crate::startup::with_task_table;
use crate::task;
use abi::{FaultInfo, FaultSource};

extern crate riscv_rt;

use riscv::register;
use riscv::register::mcause::{Exception, Interrupt, Trap};

use crate::arch::apply_memory_protection;
use crate::arch::{get_current_task, set_current_task};
use crate::arch::{incr_ticks, reset_timer};

// Provide our own interrupt vector to handle save/restore of the task on
// entry, overwriting the symbol set up by riscv-rt.  The repr(align(4)) is
// necessary as the bottom bits are used to determine direct or vectored traps.
//
// We may want to switch to a vectored interrupt table at some point to improve
// performance.
#[naked]
#[no_mangle]
#[repr(align(8))]
#[link_section = ".trap.rust"]
#[export_name = "_start_trap"]
pub unsafe extern "C" fn _start_trap() {
    unsafe {
        asm!(
            "
        #
        # Store full task status on entry, setting up a0 to point at our
        # current task so that it's passed into our exception handler.
        #
        # mscratch temporarily doesn't point to current task since it is
        # used to stash a0. mscratch is restored to current task pointer
        # just before jump to trap_handler.
        #
        csrrw a0, mscratch, a0    # mscratch no longer holds current task ptr
        sd ra,   0*8(a0)
        sd sp,   1*8(a0)
        sd gp,   2*8(a0)
        sd tp,   3*8(a0)
        sd t0,   4*8(a0)
        sd t1,   5*8(a0)
        sd t2,   6*8(a0)
        sd s0,   7*8(a0)
        sd s1,   8*8(a0)
        #sd a0,  9*8(a0)
        sd a1,  10*8(a0)
        sd a2,  11*8(a0)
        sd a3,  12*8(a0)
        sd a4,  13*8(a0)
        sd a5,  14*8(a0)
        sd a6,  15*8(a0)
        sd a7,  16*8(a0)
        sd s2,  17*8(a0)
        sd s3,  18*8(a0)
        sd s4,  19*8(a0)
        sd s5,  20*8(a0)
        sd s6,  21*8(a0)
        sd s7,  22*8(a0)
        sd s8,  23*8(a0)
        sd s9,  24*8(a0)
        sd s10, 25*8(a0)
        sd s11, 26*8(a0)
        sd t3,  27*8(a0)
        sd t4,  28*8(a0)
        sd t5,  29*8(a0)
        sd t6,  30*8(a0)
        csrr a1, mepc
        sd a1,  31*8(a0)    # store mepc for resume
        csrrw a1, mscratch, a0        # current task ptr restored in mscratch
        sd a1, 9*8(a0)      # store a0 itself

        #
        # Jump to our main rust handler
        #
        jal trap_handler

        #
        # On the way out we may have switched to a different task, load
        # everything in and resume (using t6 as it's resored last).
        #
        csrr t6, mscratch

        ld t5,  31*8(t6)     # restore mepc
        csrw mepc, t5

        ld ra,   0*8(t6)
        ld sp,   1*8(t6)
        ld gp,   2*8(t6)
        ld tp,   3*8(t6)
        ld t0,   4*8(t6)
        ld t1,   5*8(t6)
        ld t2,   6*8(t6)
        ld s0,   7*8(t6)
        ld s1,   8*8(t6)
        ld a0,   9*8(t6)
        ld a1,  10*8(t6)
        ld a2,  11*8(t6)
        ld a3,  12*8(t6)
        ld a4,  13*8(t6)
        ld a5,  14*8(t6)
        ld a6,  15*8(t6)
        ld a7,  16*8(t6)
        ld s2,  17*8(t6)
        ld s3,  18*8(t6)
        ld s4,  19*8(t6)
        ld s5,  20*8(t6)
        ld s6,  21*8(t6)
        ld s7,  22*8(t6)
        ld s8,  23*8(t6)
        ld s9,  24*8(t6)
        ld s10, 25*8(t6)
        ld s11, 26*8(t6)
        ld t3,  27*8(t6)
        ld t4,  28*8(t6)
        ld t5,  29*8(t6)
        ld t6,  30*8(t6)

        mret
        ",
            options(noreturn),
        );
    }
}

#[no_mangle]
fn timer_handler() {
    crate::profiling::event_timer_isr_enter();
    unsafe {
        with_task_table(|tasks| {
            // Advance the kernel's notion of time.
            // This increment is not expected to overflow in a working system, since it
            // would indicate that 2^64 ticks have passed, and ticks are expected to be
            // in the range of nanoseconds to milliseconds -- meaning over 500 years.
            // However, we do not use wrapping add here because, if we _do_ overflow due
            // to e.g. memory corruption, we'd rather panic and reboot than attempt to
            // limp forward.
            let now = incr_ticks(1);

            // Process any timers.
            let switch = task::process_timers(tasks, now);

            if switch != task::NextTask::Same {
                // Safety: we can access this by virtue of being an interrupt
                // handler, and thus serialized with respect to anyone who might be
                // trying to write it.
                let current = get_current_task();

                // Safety: we're dereferencing the current task pointer, which we're
                // trusting the rest of this module to maintain correctly.
                let current = usize::from(current.descriptor().index);

                let next = task::select(current, tasks);
                let next = &mut tasks[next];
                apply_memory_protection(next);
                // Safety: next comes from teh task table and we don't use it again
                // until next kernel entry, so we meet the function requirements.
                set_current_task(next);
            }

            // Reset mtime back to 0.  In theory we could save an instruction on
            // RV32 here and only write the low-order bits, assuming that it has
            // been less than 12 seconds or so since our last interrupt(!), but
            // let's avoid any possibility of a nasty surprise.
            reset_timer();
        })
    }
    crate::profiling::event_timer_isr_exit();
}

//
// The Rust side of our trap handler after the task's registers have been
// saved to SavedState.
//
#[no_mangle]
fn trap_handler(task: &mut task::Task) {
    let cause = register::mcause::read().cause();
    match cause {
        //
        // Interrupts.  Only our periodic MachineTimer interrupt via mtime is
        // supported at present.
        //
        Trap::Interrupt(Interrupt::MachineTimer) => {
            timer_handler();
        }
        //
        // System Calls.
        //
        Trap::Exception(Exception::UserEnvCall) => {
            unsafe {
                // Advance program counter past ecall instruction.
                let epc = register::mepc::read() as u64 + 4;
                task.save_mut().set_pc(epc);
                asm!(
                    "
                    mv a0, a7               # arg0 = syscall number
                    csrr a1, mscratch       # arg1 = task ptr
                    jal syscall_entry
                    ",
                );
            }
        }
        //
        // Exceptions.  Routed via the most appropriate FaultInfo.
        //
        Trap::Exception(Exception::IllegalInstruction) => unsafe {
            handle_fault(task, FaultInfo::IllegalInstruction);
        },
        Trap::Exception(Exception::LoadFault) => unsafe {
            handle_fault(
                task,
                FaultInfo::MemoryAccess {
                    address: Some(register::mtval::read() as usize),
                    source: FaultSource::User,
                },
            );
        },
        Trap::Exception(Exception::InstructionFault) => unsafe {
            handle_fault(task, FaultInfo::IllegalText);
        },
        _ => {
            panic!("Unimplemented exception {:x?}!", cause);
        }
    }
}

#[no_mangle]
unsafe fn handle_fault(task: *mut task::Task, fault: FaultInfo) {
    // Safety: we're dereferencing the current taask pointer, which we're
    // trusting the restof this module to maintain correctly.
    let idx = usize::from(unsafe { (*task).descriptor().index });
    unsafe {
        with_task_table(|tasks| {
            let next = match task::force_fault(tasks, idx, fault) {
                task::NextTask::Specific(i) => i,
                task::NextTask::Other => task::select(idx, tasks),
                task::NextTask::Same => idx,
            };

            if next == idx {
                panic!("attempt to return to Task #{} after fault", idx);
            }

            let next = &mut tasks[next];
            apply_memory_protection(next);
            // Safety: this leaks a pointer aliasing into static scope, but
            // we're not going to read it back until the next kernel entry so
            // we won't be aliasing/racing.
            set_current_task(next);
        });
    }
}

#[allow(unused_variables)]
pub fn disable_irq(n: u32) {}

#[allow(unused_variables)]
pub fn enable_irq(n: u32) {}
