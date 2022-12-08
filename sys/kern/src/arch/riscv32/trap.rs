// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
use crate::arch::{reset_timer, CURRENT_TASK_PTR, TICKS};

use crate::startup::with_task_table;
use crate::task;
use crate::time::Timestamp;

use abi::{FaultInfo, FaultSource};
use core::arch::asm;

use riscv::register;
use riscv::register::mcause::{Exception, Interrupt, Trap};

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

// Provide our own interrupt vector to handle save/restore of the task on
// entry, overwriting the symbol set up by riscv-rt.  The repr(align(4)) is
// necessary as the bottom bits are used to determine direct or vectored traps.
//
// We may want to switch to a vectored interrupt table at some point to improve
// performance.
#[naked]
#[no_mangle]
#[repr(align(4))]
#[link_section = ".trap.rust"]
#[export_name = "_start_trap"]
/// # Safety
/// `trap_handler` takes a single argument, the current task pointer, that
/// is loaded into `a0` at the beginning of this function. Additionally, this
/// function is only ever called by the core, so there shouldn't be any issues
/// with this being called by software. And because the context save and restore
/// are in the correct order (which can be verified visually), the only
/// unresolved issue of safety is the validity of CURRENT_TASK_PTR. If this
/// were to not be a correct value, then there would be undefined behaviour.
///
/// Basically, this function is safe if the core jumps to it on a trap. If this
/// were to be called by any other means it would result in undefined behavior.
unsafe extern "C" fn _start_trap() {
    unsafe {
        asm!(
            "
        #
        # Store full task status on entry, setting up a0 to point at our
        # current task so that it's passed into our exception handler.
        #
        csrw mscratch, a0
        la a0, CURRENT_TASK_PTR
        lw a0, (a0)
        sw ra,   0*4(a0)
        sw sp,   1*4(a0)
        sw gp,   2*4(a0)
        sw tp,   3*4(a0)
        sw t0,   4*4(a0)
        sw t1,   5*4(a0)
        sw t2,   6*4(a0)
        sw s0,   7*4(a0)
        sw s1,   8*4(a0)
        #sw a0,  9*4(a0)
        sw a1,  10*4(a0)
        sw a2,  11*4(a0)
        sw a3,  12*4(a0)
        sw a4,  13*4(a0)
        sw a5,  14*4(a0)
        sw a6,  15*4(a0)
        sw a7,  16*4(a0)
        sw s2,  17*4(a0)
        sw s3,  18*4(a0)
        sw s4,  19*4(a0)
        sw s5,  20*4(a0)
        sw s6,  21*4(a0)
        sw s7,  22*4(a0)
        sw s8,  23*4(a0)
        sw s9,  24*4(a0)
        sw s10, 25*4(a0)
        sw s11, 26*4(a0)
        sw t3,  27*4(a0)
        sw t4,  28*4(a0)
        sw t5,  29*4(a0)
        sw t6,  30*4(a0)
        csrr a1, mepc
        sw a1,  31*4(a0)    # store mepc for resume
        csrr a1, mscratch
        sw a1, 9*4(a0)      # store a0 itself

        #
        # Jump to our main rust handler
        #
        jal trap_handler

        #
        # On the way out we may have switched to a different task, load
        # everything in and resume (using t6 as it's resored last).
        #
        la t6, CURRENT_TASK_PTR
        lw t6, (t6)

        lw t5,  31*4(t6)     # restore mepc
        csrw mepc, t5

        lw ra,   0*4(t6)
        lw sp,   1*4(t6)
        lw gp,   2*4(t6)
        lw tp,   3*4(t6)
        lw t0,   4*4(t6)
        lw t1,   5*4(t6)
        lw t2,   6*4(t6)
        lw s0,   7*4(t6)
        lw s1,   8*4(t6)
        lw a0,   9*4(t6)
        lw a1,  10*4(t6)
        lw a2,  11*4(t6)
        lw a3,  12*4(t6)
        lw a4,  13*4(t6)
        lw a5,  14*4(t6)
        lw a6,  15*4(t6)
        lw a7,  16*4(t6)
        lw s2,  17*4(t6)
        lw s3,  18*4(t6)
        lw s4,  19*4(t6)
        lw s5,  20*4(t6)
        lw s6,  21*4(t6)
        lw s7,  22*4(t6)
        lw s8,  23*4(t6)
        lw s9,  24*4(t6)
        lw s10, 25*4(t6)
        lw s11, 26*4(t6)
        lw t3,  27*4(t6)
        lw t4,  28*4(t6)
        lw t5,  29*4(t6)
        lw t6,  30*4(t6)

        mret
        ",
            options(noreturn),
        );
    }
}

#[no_mangle]
fn timer_handler() {
    crate::profiling::event_timer_isr_enter();
    let ticks = unsafe { &mut TICKS };
    unsafe {
        with_task_table(|tasks| {
            // Advance the kernel's notion of time.
            // This increment is not expected to overflow in a working system, since it
            // would indicate that 2^64 ticks have passed, and ticks are expected to be
            // in the range of nanoseconds to milliseconds -- meaning over 500 years.
            // However, we do not use wrapping add here because, if we _do_ overflow due
            // to e.g. memory corruption, we'd rather panic and reboot than attempt to
            // limp forward.
            *ticks += 1;
            // Now, give up mutable access to *ticks so there's no chance of a
            // double-increment due to bugs below.
            let now = Timestamp::from(*ticks);

            // Process any timers.
            let switch = task::process_timers(tasks, now);

            if switch != task::NextTask::Same {
                // Safety: we can access this by virtue of being an interrupt
                // handler, and thus serialized with respect to anyone who might be
                // trying to write it.
                let current = CURRENT_TASK_PTR
                    .expect("irq before kernel started?")
                    .as_ptr();

                // Safety: we're dereferencing the current task pointer, which we're
                // trusting the rest of this module to maintain correctly.
                let current = usize::from((*current).descriptor().index);

                let next = task::select(current, tasks);
                let next = &mut tasks[next];
                // Safety: next comes from the task table and we don't use it again
                // until next kernel entry, so we meet the function requirements.
                crate::task::activate_next_task(next);
            }

            //
            // Increase mtimecmp for the next interrupt
            // This will also clear the pending timer interrupt.
            //
            reset_timer();
        })
    }
    crate::profiling::event_timer_isr_exit();
}

// Handler for interrupts related to the platform
#[no_mangle]
fn platform_interrupt_handler(irq: u32) {
    let owner = crate::startup::HUBRIS_IRQ_TASK_LOOKUP
        .get(abi::InterruptNum(irq as u32))
        .unwrap_or_else(|| panic!("unhandled IRQ {}", irq));

    let switch: bool = with_task_table(|tasks| {
        disable_irq(irq as u32);

        // Now, post the notification and return the
        // scheduling hint.
        let n = task::NotificationSet(owner.notification);
        tasks[owner.task as usize].post(n)
    });

    if switch {
        // Safety: we can access this by virtue of being an interrupt handler, and
        // thus serialized with respect to anyone who might be trying to write it.
        let current = unsafe { CURRENT_TASK_PTR }
            .expect("irq before kernel started?")
            .as_ptr();

        // Safety: we're dereferencing the current task pointer, which we're
        // trusting the rest of this module to maintain correctly.
        let current = usize::from(unsafe { (*current).descriptor().index });

        unsafe {
            with_task_table(|tasks| {
                let next = task::select(current, tasks);
                let next = &mut tasks[next];
                // Safety: next comes from the task table and we don't use it again
                // until next kernel entry, so we meet the function requirements.
                crate::task::activate_next_task(next);
            })
        };
    }

    disable_irq(irq);
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
            // Safety: next comes from the task table and we don't use it again
            // until next kernel entry, so we meet the function requirements.
            crate::task::activate_next_task(next);
        });
    }
}

//
// The Rust side of our trap handler after the task's registers have been
// saved to SavedState.
//
#[no_mangle]
fn trap_handler(task: &mut task::Task) {
    let mcause = register::mcause::read();
    match mcause.cause() {
        //
        // Interrupts.  Only our periodic MachineTimer interrupt via mtime is
        // supported at present.
        //
        Trap::Interrupt(Interrupt::MachineTimer) => {
            timer_handler();
        }

        //
        // External Interrupts
        //
        Trap::Interrupt(Interrupt::MachineExternal) => {
            platform_interrupt_handler(11);
        }
        //
        // System Calls.
        //
        Trap::Exception(Exception::UserEnvCall) => {
            unsafe {
                // Advance program counter past ecall instruction.
                task.save_mut().set_pc(register::mepc::read() as u32 + 4);
                asm!(
                    "
                    la a1, CURRENT_TASK_PTR
                    mv a0, a7               # arg0 = syscall number
                    lw a1, (a1)             # arg1 = task ptr
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
        Trap::Exception(Exception::LoadFault)
        | Trap::Exception(Exception::StoreFault) => unsafe {
            handle_fault(
                task,
                FaultInfo::MemoryAccess {
                    address: Some(register::mtval::read() as u32),
                    source: FaultSource::User,
                },
            );
        },
        Trap::Exception(Exception::InstructionFault) => unsafe {
            handle_fault(task, FaultInfo::IllegalText);
        },

        _ => {
            cfg_if::cfg_if! {
                if #[cfg(feature = "custom-interrupts")] {
                    if mcause.is_exception() == true {
                        panic!("Unimplemented exception");
                    } else if mcause.code() >= 16 {
                        platform_interrupt_handler(
                            mcause
                                .code()
                                .try_into()
                                .expect("Recieved an interrupt with cause >= 2^32"),
                        );
                    }
                } else {
                    panic!("Unimplemented trap");
                }
            }
        }
    }
}

pub fn disable_irq(n: u32) {
    let cur_mie = register::mie::read();
    let new_mie = cur_mie.bits() & !(0x1 << n);
    unsafe {
        asm!("
            csrw mie, {x}",
            x = in(reg) new_mie,
        );
    }
}

pub fn enable_irq(n: u32) {
    let cur_mie = register::mie::read();
    let new_mie = cur_mie.bits() | (0x1 << n);
    unsafe {
        asm!("
            csrw mie, {x}",
            x = in(reg) new_mie,
        );
    }
}
