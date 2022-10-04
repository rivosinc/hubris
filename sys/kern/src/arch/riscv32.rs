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

use crate::startup::with_task_table;
use crate::task;
use crate::time::Timestamp;
use crate::umem::USlice;
use abi::{FaultInfo, FaultSource};
use unwrap_lite::UnwrapLite;

extern crate riscv_rt;

use riscv::register;
use riscv::register::mcause::{Exception, Interrupt, Trap};
use riscv::register::mstatus::MPP;

#[allow(unused)]
macro_rules! uassert {
    ($cond : expr) => {
        if !$cond {
            panic!("Assertion failed!");
        }
    };
}

/// On RISC-V we use a global to record the current task pointer.  It may be
/// possible to use the mscratch register instead.
#[no_mangle]
static mut CURRENT_TASK_PTR: Option<NonNull<task::Task>> = None;

/// To allow our clock frequency to be easily determined from a debugger, we
/// store it in memory.
#[no_mangle]
static mut CLOCK_FREQ_KHZ: u32 = 0;

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
pub unsafe fn set_clock_freq(tick_divisor: u32) {
    // TODO switch me to an atomic. Note that this may break Humility.
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

#[allow(unused_variables)]
pub fn apply_memory_protection(task: &task::Task) {
    use riscv::register::{Mode, Permission, PmpCfg};

    let null_cfg: PmpCfg = PmpCfg::new(Mode::OFF, Permission::NONE, false);

    for (i, region) in task.region_table().iter().enumerate() {
        let pmpcfg =
            // Check if this is a filler region
            if (region.base == 0x0) && (region.size == 0x20) {
                null_cfg
            } else {
                let pmp_perm: Permission = match region.attributes.bits() & 0b111 {
                    0b000 => Permission::NONE,
                    0b001 => Permission::R,
                    0b010 => panic!(),
                    0b011 => Permission::RW,
                    0b100 => Permission::X,
                    0b101 => Permission::RX,
                    0b110 => panic!(),
                    0b111 => Permission::RWX,
                    _ => unreachable!(),
                };

                PmpCfg::new(Mode::TOR, pmp_perm, false)
            };

        unsafe {
            // Configure the base address entry
            register::set_cfg_entry(i * 2, null_cfg);
            register::write_tor_indexed(i * 2, region.base as usize);

            // Configure the end address entry
            register::set_cfg_entry(i * 2 + 1, pmpcfg);
            register::write_tor_indexed(
                i * 2 + 1,
                (region.base + region.size) as usize,
            );
        }
    }
}

cfg_if::cfg_if! {
    if #[cfg(feature = "vectored-interrupts")] {
        use riscv::register::mtvec::{self, TrapMode};

        // Setup interrupt vector `mtvec` with vectored mode to the trap table.
        // SAFETY: if _start_trap does not have the neccasary alignment,
        // the address could become corrupt and traps will not jump to the
        // expected address
        #[export_name = "_setup_interrupts"]
        pub unsafe extern "C" fn _setup_interrputs() {
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
        pub unsafe extern "C" fn _trap_table() {
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
pub unsafe extern "C" fn _start_trap() {
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
            drop(ticks);

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
                // Safety: next comes from teh task table and we don't use it again
                // until next kernel entry, so we meet the function requirements.
                crate::task::switch_to(next);
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

    if switch == true {
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
                // Safety: next comes from teh task table and we don't use it again
                // until next kernel entry, so we meet the function requirements.
                crate::task::switch_to(next);
            })
        };
    }

    disable_irq(irq);
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
                task.save_mut().pc = register::mepc::read() as u32 + 4;
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
        Trap::Exception(Exception::LoadFault) => unsafe {
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
            // Safety: next comes from teh task table and we don't use it again
            // until next kernel entry, so we meet the function requirements.
            crate::task::switch_to(next);
        });
    }
}

// Timer handling.
//
// We currently only support single HART systems.  From reading elsewhere,
// additional harts have their own mtimecmp offset at 0x8 intervals from hart0.
//
// Configure the timer.
//
// RISC-V Privileged Architecture Manual
// 3.2.1 Machine Timer Registers (mtime and mtimecmp)
//
unsafe fn reset_timer() {
    //
    // Increase mtimecmp for the next interrupt
    // This will also clear the pending timer interrupt.
    //
    unsafe {
        let mut mtimecmp =
            core::ptr::read_volatile(crate::startup::MTIMECMP as *mut u64);
        mtimecmp = mtimecmp + CLOCK_FREQ_KHZ as u64;
        core::ptr::write_volatile(
            crate::startup::MTIMECMP as *mut u64,
            mtimecmp,
        );
    }
}

#[allow(unused_variables)]
pub fn start_first_task(tick_divisor: u32, task: &task::Task) -> ! {
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
        CURRENT_TASK_PTR = Some(NonNull::from(task));
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
    crate::profiling::event_context_switch(task as *mut _ as usize);
    // Safety: should be ok if the contract above is met
    // TODO: make me an atomic
    unsafe {
        CURRENT_TASK_PTR = Some(NonNull::from(task));
    }
}

#[used]
static mut TICKS: u64 = 0;

/// Reads the tick counter.
pub fn now() -> Timestamp {
    Timestamp::from(unsafe { TICKS })
}

pub fn reset() -> ! {
    unimplemented!();
}

// Constants that may change depending on configuration
include!(concat!(env!("OUT_DIR"), "/consts.rs"));
