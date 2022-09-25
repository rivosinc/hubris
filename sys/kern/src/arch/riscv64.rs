// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Architecture support for RISC-V.
//!
//! Interrupts (other than the Machine Timer used to advance the kernel
//! timestamp) are not yet supported.

use core::arch::asm;
use core::sync::atomic::{AtomicBool, Ordering};

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

macro_rules! uassert {
    ($cond : expr) => {
        if !$cond {
            panic!("Assertion failed!");
        }
    };
}

/// To allow our clock frequency to be easily determined from a debugger, we
/// store it in memory.
#[no_mangle]
static mut CLOCK_FREQ_KHZ: u32 = 0;

/// RISC-V volatile registers that must be saved across context switches.
#[repr(C)]
#[derive(Clone, Debug, Default, FromBytes)]
pub struct SavedState {
    // NOTE: the following fields must be kept contiguous!
    ra: u64,
    sp: u64,
    gp: u64,
    tp: u64,
    t0: u64,
    t1: u64,
    t2: u64,
    s0: u64,
    s1: u64,
    a0: u64,
    a1: u64,
    a2: u64,
    a3: u64,
    a4: u64,
    a5: u64,
    a6: u64,
    a7: u64,
    s2: u64,
    s3: u64,
    s4: u64,
    s5: u64,
    s6: u64,
    s7: u64,
    s8: u64,
    s9: u64,
    s10: u64,
    s11: u64,
    t3: u64,
    t4: u64,
    t5: u64,
    t6: u64,
    // Additional save value for task program counter
    pc: u64,
    // NOTE: the above fields must be kept contiguous!
}

/// Map the volatile registers to (architecture-independent) syscall argument
/// and return slots.
impl task::ArchState for SavedState {
    fn stack_pointer(&self) -> usize {
        self.sp as usize
    }

    /// Reads syscall argument register 0.
    fn arg0(&self) -> usize {
        self.a0 as usize
    }
    fn arg1(&self) -> usize {
        self.a1 as usize
    }
    fn arg2(&self) -> usize {
        self.a2 as usize
    }
    fn arg3(&self) -> usize {
        self.a3 as usize
    }
    fn arg4(&self) -> usize {
        self.a4 as usize
    }
    fn arg5(&self) -> usize {
        self.a5 as usize
    }
    fn arg6(&self) -> usize {
        self.a6 as usize
    }

    fn syscall_descriptor(&self) -> usize {
        self.a7 as usize
    }

    /// Writes syscall return argument 0.
    fn ret0(&mut self, x: usize) {
        self.a0 = x as u64
    }
    fn ret1(&mut self, x: usize) {
        self.a1 = x as u64
    }
    fn ret2(&mut self, x: usize) {
        self.a2 = x as u64
    }
    fn ret3(&mut self, x: usize) {
        self.a3 = x as u64
    }
    fn ret4(&mut self, x: usize) {
        self.a4 = x as u64
    }
    fn ret5(&mut self, x: usize) {
        self.a5 = x as u64
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
    // the RISC-V callineg convention.
    let initial_stack: usize = task.descriptor().initial_stack;
    task.save_mut().sp = initial_stack as u64;
    uassert!(task.save().sp & 0xF == 0);

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
    task.save_mut().pc = task.descriptor().entry_point as u64;
}

pub fn apply_memory_protection(task: &task::Task) {
    use riscv::register::{Mode, Permission, PmpCfg};

    let null_cfg: PmpCfg = PmpCfg::new(Mode::OFF, Permission::NONE, false);

    for (i, region) in task.region_table().iter().enumerate() {
        if (region.base == 0x0) && (region.size == 0x20) {
            continue;
        }
        let pmpcfg = {
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
        csrrw a0, mscratch, a0
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
        csrr a1, mscratch
        sd a1, 9*8(a0)      # store a0 itself
        csrw mscratch, a0   # restore task ptr in mscratch

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
            core::ptr::write_volatile(MTIME as *mut u64, 0);
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
                task.save_mut().pc = register::mepc::read() as u64 + 4;
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

// Timer handling.
//
// We currently only support single HART systems.  From reading elsewhere,
// additional harts have their own mtimecmp offset at 0x8 intervals from hart0.
//
// As per FE310-G002 Manual, section 9.1, the address of mtimecmp on
// our supported board is 0x0200_4000, which also matches qemu.
//
// On both RV32 and RV64 systems the mtime and mtimecmp memory-mapped registers
// are 64-bits wide.
//
const MTIMECMP: u64 = 0x0200_4000;
const MTIME: u64 = 0x0200_BFF8;

// Configure the timer.
//
// RISC-V Privileged Architecture Manual
// 3.2.1 Machine Timer Registers (mtime and mtimecmp)
//
// To keep things simple, especially on RV32 systems where we cannot atomically
// write to the mtime/mtimecmp memory-mapped registers as they are 64 bits
// wide, we only utilise the first 32-bits of each register, setting the
// high-order bits to 0 on startup, and restarting the low-order bits of mtime
// back to 0 on each interrupt.
//
#[no_mangle]
unsafe fn set_timer(tick_divisor: u32) {
    // Set high-order bits of mtime to zero.  We only call this function prior
    // to enabling interrupts so it should be safe.
    unsafe {
        asm!("
        li {0}, {mtimecmp}  # load mtimecmp address
        sd {1}, 0({0})      # set mtimecmp register

        li {0}, {mtime}     # load mtime address
        sd zero, 0({0})     # set low-order bits back to 0
        ",
            out(reg) _,
            in(reg) tick_divisor,
            mtime = const MTIME,
            mtimecmp = const MTIMECMP,
        );
    }
}

#[allow(unused_variables)]
pub fn start_first_task(tick_divisor: u32, task: &task::Task) -> ! {
    // Configure MPP to switch us to User mode on exit from Machine
    // mode (when we call "mret" below).
    unsafe {
        register::mstatus::set_mpp(MPP::User);
    }

    // Write the initial task program counter.
    register::mepc::write(task.save().pc as *const usize as usize);

    //
    // Configure the timer
    //
    unsafe {
        CLOCK_FREQ_KHZ = tick_divisor;

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
            ld sp, ({sp})
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

unsafe fn get_current_task() -> &'static task::Task {
    let mut task: usize;
    unsafe {
        asm!("csrr {0}, mscratch", out(reg) task);
        core::mem::transmute::<usize, &task::Task>(task)
    }
}

#[used]
static mut TICKS: u64 = 0;

/// Reads the tick counter.
pub fn now() -> Timestamp {
    Timestamp::from(unsafe { TICKS })
}

#[allow(unused_variables)]
pub fn disable_irq(n: u32) {}

#[allow(unused_variables)]
pub fn enable_irq(n: u32) {}

pub fn reset() -> ! {
    unimplemented!();
}

// Constants that may change depending on configuration
include!(concat!(env!("OUT_DIR"), "/consts.rs"));
