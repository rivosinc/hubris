// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! User application architecture support stubs for ARMv7m/ARMv8m.
//!
//! See the note on syscall stubs at the top of the userlib module for
//! rationale.

use crate::*;

/// This is the entry point for the task, invoked by the kernel. Its job is to
/// set up our memory before jumping to user-defined `main`.
#[doc(hidden)]
#[no_mangle]
#[link_section = ".text.start"]
#[naked]
pub unsafe extern "C" fn _start() -> ! {
    // Provided by the user program:
    extern "Rust" {
        fn main() -> !;
    }

    cfg_if::cfg_if! {
        if #[cfg(armv6m)] {
            core::arch::asm!("
                @ Copy data initialization image into data section.
                @ Note: this assumes that both source and destination are 32-bit
                @ aligned and padded to 4-byte boundary.

                ldr r0, =__edata            @ upper bound in r0
                ldr r1, =__sidata           @ source in r1
                ldr r2, =__sdata            @ dest in r2

                b 1f                        @ check for zero-sized data

            2:  ldm r1!, {{r3}}             @ read and advance source
                stm r2!, {{r3}}             @ write and advance dest

            1:  cmp r2, r0                  @ has dest reached the upper bound?
                bne 2b                      @ if not, repeat

                @ Zero BSS section.

                ldr r0, =__ebss             @ upper bound in r0
                ldr r1, =__sbss             @ base in r1

                movs r2, #0                 @ materialize a zero

                b 1f                        @ check for zero-sized BSS

            2:  stm r1!, {{r2}}             @ zero one word and advance

            1:  cmp r1, r0                  @ has base reached bound?
                bne 2b                      @ if not, repeat

                @ Be extra careful to ensure that those side effects are
                @ visible to the user program.

                dsb         @ complete all writes
                isb         @ and flush the pipeline

                @ Now, to the user entry point. We call it in case it
                @ returns. (It's not supposed to.) We reference it through
                @ a sym operand because it's a Rust func and may be mangled.
                bl {main}

                @ The noreturn option below will automatically generate an
                @ undefined instruction trap past this point, should main
                @ return.
                ",
                main = sym main,
                options(noreturn),
            )
        } else if #[cfg(any(armv7m, armv8m))] {
            core::arch::asm!("
                @ Copy data initialization image into data section.
                @ Note: this assumes that both source and destination are 32-bit
                @ aligned and padded to 4-byte boundary.

                movw r0, #:lower16:__edata  @ upper bound in r0
                movt r0, #:upper16:__edata

                movw r1, #:lower16:__sidata @ source in r1
                movt r1, #:upper16:__sidata

                movw r2, #:lower16:__sdata  @ dest in r2
                movt r2, #:upper16:__sdata

                b 1f                        @ check for zero-sized data

            2:  ldr r3, [r1], #4            @ read and advance source
                str r3, [r2], #4            @ write and advance dest

            1:  cmp r2, r0                  @ has dest reached the upper bound?
                bne 2b                      @ if not, repeat

                @ Zero BSS section.

                movw r0, #:lower16:__ebss   @ upper bound in r0
                movt r0, #:upper16:__ebss

                movw r1, #:lower16:__sbss   @ base in r1
                movt r1, #:upper16:__sbss

                movs r2, #0                 @ materialize a zero

                b 1f                        @ check for zero-sized BSS

            2:  str r2, [r1], #4            @ zero one word and advance

            1:  cmp r1, r0                  @ has base reached bound?
                bne 2b                      @ if not, repeat

                @ Be extra careful to ensure that those side effects are
                @ visible to the user program.

                dsb         @ complete all writes
                isb         @ and flush the pipeline

                @ Now, to the user entry point. We call it in case it
                @ returns. (It's not supposed to.) We reference it through
                @ a sym operand because it's a Rust func and may be mangled.
                bl {main}

                @ The noreturn option below will automatically generate an
                @ undefined instruction trap past this point, should main
                @ return.
                ",
                main = sym main,
                options(noreturn),
            )
        } else {
            compile_error!("missing .start routine for ARM profile")
        }
    }
}

/// Core implementation of the REFRESH_TASK_ID syscall.
#[naked]
pub(crate) unsafe extern "C" fn sys_refresh_task_id_stub(_tid: u32) -> u32 {
    cfg_if::cfg_if! {
        if #[cfg(armv6m)] {
            core::arch::asm!("
                @ Spill the registers we're about to use to pass stuff.
                @ match!
                push {{r4, r5, lr}}
                mov r4, r11
                push {{r4}}

                @ Load the constant syscall number.
                movs r4, #0
                adds r4, #{sysnum}
                mov r11, r4

                @ Move register arguments into place.
                mov r4, r0

                @ To the kernel!
                svc #0

                @ Move result into place.
                mov r0, r4

                @ Restore the registers we used and return.
                pop {{r4}}
                mov r11, r4
                pop {{r4, r5, pc}}
                ",
                sysnum = const Sysnum::RefreshTaskId as u32,
                options(noreturn),
            )
        } else if #[cfg(any(armv7m, armv8m))] {
            core::arch::asm!("
                @ Spill the registers we're about to use to pass stuff.
                push {{r4, r5, r11, lr}}

                @ Move register arguments into place.
                mov r4, r0
                @ Load the constant syscall number.
                mov r11, {sysnum}

                @ To the kernel!
                svc #0

                @ Move result into place.
                mov r0, r4

                @ Restore the registers we used and return.
                pop {{r4, r5, r11, pc}}
                ",
                sysnum = const Sysnum::RefreshTaskId as u32,
                options(noreturn),
            )
        } else {
            compile_error!("missing sys_refresh_task_id stub for ARM profile")
        }
    }
}

/// Core implementation of the SEND syscall.
#[naked]
pub(crate) unsafe extern "C" fn sys_send_stub(
    _args: &mut SendArgs<'_>,
) -> RcLen {
    cfg_if::cfg_if! {
        if #[cfg(armv6m)] {
            core::arch::asm!("
                @ Spill the registers we're about to use to pass stuff.
                push {{r4-r7, lr}}
                mov r4, r8
                mov r5, r9
                mov r6, r10
                mov r7, r11
                push {{r4-r7}}
                @ Load the constant syscall number.
                eors r4, r4
                adds r4, #{sysnum}
                mov r11, r4
                @ Load in args from the struct.
                ldm r0!, {{r5-r7}}
                ldm r0!, {{r1-r4}}
                mov r8, r1
                mov r9, r2
                mov r10, r3

                @ To the kernel!
                svc #0

                @ Move the two results back into their return positions.
                mov r0, r4
                mov r1, r5
                @ Restore the registers we used.
                pop {{r4-r7}}
                mov r8, r4
                mov r9, r5
                mov r10, r6
                mov r11, r7
                pop {{r4-r7, pc}}
                ",
                sysnum = const Sysnum::Send as u32,
                options(noreturn),
            )
        } else if #[cfg(any(armv7m, armv8m))] {
            core::arch::asm!("
                @ Spill the registers we're about to use to pass stuff.
                push {{r4-r11}}
                @ Load in args from the struct.
                ldm r0!, {{r5-r10}}
                ldm r0, {{r4}}
                @ Load the constant syscall number.
                mov r11, {sysnum}

                @ To the kernel!
                svc #0

                @ Move the two results back into their return positions.
                mov r0, r4
                mov r1, r5
                @ Restore the registers we used.
                pop {{r4-r11}}
                @ Fin.
                bx lr
                ",
                sysnum = const Sysnum::Send as u32,
                options(noreturn),
            )
        } else {
            compile_error!("missing sys_send_stub for ARM profile");
        }
    }
}

/// Core implementation of the RECV syscall.
#[naked]
#[must_use]
pub(crate) unsafe extern "C" fn sys_recv_stub(
    _buffer_ptr: *mut u8,
    _buffer_len: usize,
    _notification_mask: u32,
    _specific_sender: u32,
    _out: *mut RawRecvMessage,
) -> u32 {
    cfg_if::cfg_if! {
        if #[cfg(armv6m)] {
            core::arch::asm!("
                @ Spill the registers we're about to use to pass stuff.
                push {{r4-r7, lr}}
                mov r4, r8
                mov r5, r9
                mov r6, r10
                mov r7, r11
                push {{r4-r7}}
                @ Load the constant syscall number.
                eors r4, r4
                adds r4, #{sysnum}
                mov r11, r4
                @ Move register arguments into their proper positions.
                mov r4, r0
                mov r5, r1
                mov r6, r2
                mov r7, r3
                @ Read output buffer pointer from stack into a register that
                @ is preserved during our syscall. Since we just pushed a
                @ bunch of stuff, we need to read *past* it.
                ldr r3, [sp, #(9 * 4)]

                @ To the kernel!
                svc #0

                @ Move status flag (only used for closed receive) into return
                @ position
                mov r0, r4
                @ Write all the results out into the raw output buffer.
                stm r3!, {{r5-r7}}
                mov r5, r8
                mov r6, r9
                stm r3!, {{r5-r6}}

                @ Restore the registers we used.
                pop {{r4-r7}}
                mov r8, r4
                mov r9, r5
                mov r10, r6
                mov r11, r7
                pop {{r4-r7, pc}}
                ",
                sysnum = const Sysnum::Recv as u32,
                options(noreturn),
            )
        } else if #[cfg(any(armv7m, armv8m))] {
            core::arch::asm!("
                @ Spill the registers we're about to use to pass stuff.
                push {{r4-r11}}
                @ Move register arguments into their proper positions.
                mov r4, r0
                mov r5, r1
                mov r6, r2
                mov r7, r3
                @ Read output buffer pointer from stack into a register that
                @ is preserved during our syscall. Since we just pushed a
                @ bunch of stuff, we need to read *past* it.
                ldr r3, [sp, #(8 * 4)]
                @ Load the constant syscall number.
                mov r11, {sysnum}

                @ To the kernel!
                svc #0

                @ Move status flag (only used for closed receive) into return
                @ position
                mov r0, r4
                @ Write all the results out into the raw output buffer.
                stm r3, {{r5-r9}}
                @ Restore the registers we used.
                pop {{r4-r11}}
                @ Fin.
                bx lr
                ",
                sysnum = const Sysnum::Recv as u32,
                options(noreturn),
            )
        } else {
            compile_error!("missing sys_recv_stub for ARM profile");
        }
    }
}

/// Core implementation of the REPLY syscall.
#[naked]
pub(crate) unsafe extern "C" fn sys_reply_stub(
    _peer: u32,
    _code: u32,
    _message_ptr: *const u8,
    _message_len: usize,
) {
    cfg_if::cfg_if! {
        if #[cfg(armv6m)] {
            core::arch::asm!("
                @ Spill the registers we're about to use to pass stuff. Note
                @ that we're being clever and pushing only the registers we
                @ need; this means the pop sequence at the end needs to match!
                push {{r4-r7, lr}}
                mov r4, r11
                push {{r4}}

                @ Load the constant syscall number.
                eors r4, r4
                adds r4, #{sysnum}
                mov r11, r4
                @ Move register arguments into place.
                mov r4, r0
                mov r5, r1
                mov r6, r2
                mov r7, r3

                @ To the kernel!
                svc #0

                @ This call has no results.

                @ Restore the registers we used and return.
                pop {{r4}}
                mov r11, r4
                pop {{r4-r7, pc}}
                ",
                sysnum = const Sysnum::Reply as u32,
                options(noreturn),
            )
        } else if #[cfg(any(armv7m, armv8m))] {
            core::arch::asm!("
                @ Spill the registers we're about to use to pass stuff. Note
                @ that we're being clever and pushing only the registers we
                @ need; this means the pop sequence at the end needs to match!
                push {{r4-r7, r11, lr}}

                @ Move register arguments into place.
                mov r4, r0
                mov r5, r1
                mov r6, r2
                mov r7, r3
                @ Load the constant syscall number.
                mov r11, {sysnum}

                @ To the kernel!
                svc #0

                @ This call has no results.

                @ Restore the registers we used and return.
                pop {{r4-r7, r11, pc}}
                ",
                sysnum = const Sysnum::Reply as u32,
                options(noreturn),
            )
        } else {
            compile_error!("missing sys_reply_stub for ARM profile");
        }
    }
}

/// Core implementation of the SET_TIMER syscall.
#[naked]
pub(crate) unsafe extern "C" fn sys_set_timer_stub(
    _set_timer: u32,
    _deadline_lo: u32,
    _deadline_hi: u32,
    _notification: u32,
) {
    cfg_if::cfg_if! {
        if #[cfg(armv6m)] {
            core::arch::asm!("
                @ Spill the registers we're about to use to pass stuff.
                push {{r4-r7, lr}}
                mov r4, r11
                push {{r4}}

                @ Load the constant syscall number.
                eors r4, r4
                adds r4, #{sysnum}
                mov r11, r4
                @ Move register arguments into place.
                mov r4, r0
                mov r5, r1
                mov r6, r2
                mov r7, r3

                @ To the kernel!
                svc #0

                @ This call has no results.

                @ Restore the registers we used and return.
                pop {{r4}}
                mov r11, r4
                pop {{r4-r7, pc}}
                ",
                sysnum = const Sysnum::SetTimer as u32,
                options(noreturn),
            )
        } else if #[cfg(any(armv7m, armv8m))] {
            core::arch::asm!("
                @ Spill the registers we're about to use to pass stuff.
                push {{r4-r7, r11, lr}}

                @ Move register arguments into place.
                mov r4, r0
                mov r5, r1
                mov r6, r2
                mov r7, r3
                @ Load the constant syscall number.
                mov r11, {sysnum}

                @ To the kernel!
                svc #0

                @ This call has no results.

                @ Restore the registers we used and return.
                pop {{r4-r7, r11, pc}}
                ",
                sysnum = const Sysnum::SetTimer as u32,
                options(noreturn),
            )
        } else {
            compile_error!("missing sys_set_timer_stub for ARM profile")
        }
    }
}

/// Core implementation of the BORROW_READ syscall.
///
/// See the note on syscall stubs at the top of this module for rationale.
#[naked]
pub(crate) unsafe extern "C" fn sys_borrow_read_stub(
    _args: *mut BorrowReadArgs,
) -> RcLen {
    cfg_if::cfg_if! {
        if #[cfg(armv6m)] {
            core::arch::asm!("
                @ Spill the registers we're about to use to pass stuff.
                push {{r4-r7, lr}}
                mov r4, r8
                mov r5, r11
                push {{r4, r5}}

                @ Load the constant syscall number.
                eors r4, r4
                adds r4, #{sysnum}
                mov r11, r4
                @ Move register arguments into place.
                ldm r0!, {{r5-r7}}
                ldm r0!, {{r1}}
                mov r8, r1
                ldm r0!, {{r4}}

                @ To the kernel!
                svc #0

                @ Move the results into place.
                mov r0, r4
                mov r1, r5

                @ Restore the registers we used and return.
                pop {{r4, r5}}
                mov r11, r5
                mov r8, r4
                pop {{r4-r7, pc}}
                ",
                sysnum = const Sysnum::BorrowRead as u32,
                options(noreturn),
            )
        } else if #[cfg(any(armv7m, armv8m))] {
            core::arch::asm!("
                @ Spill the registers we're about to use to pass stuff.
                push {{r4-r8, r11}}

                @ Move register arguments into place.
                ldm r0!, {{r5-r8}}
                ldm r0, {{r4}}
                @ Load the constant syscall number.
                mov r11, {sysnum}

                @ To the kernel!
                svc #0

                @ Move the results into place.
                mov r0, r4
                mov r1, r5

                @ Restore the registers we used and return.
                pop {{r4-r8, r11}}
                bx lr
                ",
                sysnum = const Sysnum::BorrowRead as u32,
                options(noreturn),
            )
        } else {
            compile_error!("missing sys_borrow_read_stub for ARM profile")
        }
    }
}

/// Core implementation of the BORROW_WRITE syscall.
#[naked]
pub(crate) unsafe extern "C" fn sys_borrow_write_stub(
    _args: *mut BorrowWriteArgs,
) -> RcLen {
    cfg_if::cfg_if! {
        if #[cfg(armv6m)] {
            core::arch::asm!("
                @ Spill the registers we're about to use to pass stuff.
                push {{r4-r7, lr}}
                mov r4, r8
                mov r5, r11
                push {{r4, r5}}

                @ Load the constant syscall number.
                eors r4, r4
                adds r4, #{sysnum}
                mov r11, r4
                @ Move register arguments into place.
                ldm r0!, {{r5-r7}}
                ldm r0, {{r1}}
                mov r8, r1
                ldm r0!, {{r4}}

                @ To the kernel!
                svc #0

                @ Move the results into place.
                mov r0, r4
                mov r1, r5

                @ Restore the registers we used and return.
                pop {{r4, r5}}
                mov r11, r5
                mov r8, r4
                pop {{r4-r7, pc}}
                bx lr
                ",
                sysnum = const Sysnum::BorrowWrite as u32,
                options(noreturn),
            )
        } else if #[cfg(any(armv7m, armv8m))] {
            core::arch::asm!("
                @ Spill the registers we're about to use to pass stuff.
                push {{r4-r8, r11}}

                @ Move register arguments into place.
                ldm r0!, {{r5-r8}}
                ldm r0, {{r4}}
                @ Load the constant syscall number.
                mov r11, {sysnum}

                @ To the kernel!
                svc #0

                @ Move the results into place.
                mov r0, r4
                mov r1, r5

                @ Restore the registers we used and return.
                pop {{r4-r8, r11}}
                bx lr
                ",
                sysnum = const Sysnum::BorrowWrite as u32,
                options(noreturn),
            )
        } else {
            compile_error!("missing sys_borrow_write_stub for ARM profile")
        }
    }
}

/// Core implementation of the BORROW_INFO syscall.
#[naked]
pub(crate) unsafe extern "C" fn sys_borrow_info_stub(
    _lender: u32,
    _index: usize,
    _out: *mut RawBorrowInfo,
) {
    cfg_if::cfg_if! {
        if #[cfg(armv6m)] {
            core::arch::asm!("
                @ Spill the registers we're about to use to pass stuff.
                push {{r4-r6, lr}}
                mov r4, r11
                push {{r4}}

                @ Load the constant syscall number.
                eors r4, r4
                adds r4, #{sysnum}
                mov r11, r4
                @ Move register arguments into place.
                mov r4, r0
                mov r5, r1

                @ To the kernel!
                svc #0

                @ Move the results into place.
                stm r2!, {{r4-r6}}

                @ Restore the registers we used and return.
                pop {{r4}}
                mov r11, r4
                pop {{r4-r6, pc}}
                ",
                sysnum = const Sysnum::BorrowInfo as u32,
                options(noreturn),
            )
        } else if #[cfg(any(armv7m, armv8m))] {
            core::arch::asm!("
                @ Spill the registers we're about to use to pass stuff.
                push {{r4-r6, r11}}

                @ Move register arguments into place.
                mov r4, r0
                mov r5, r1
                @ Load the constant syscall number.
                mov r11, {sysnum}

                @ To the kernel!
                svc #0

                @ Move the results into place.
                stm r2, {{r4-r6}}

                @ Restore the registers we used and return.
                pop {{r4-r6, r11}}
                bx lr
                ",
                sysnum = const Sysnum::BorrowInfo as u32,
                options(noreturn),
            )
        } else {
            compile_error!("missing sys_borrow_write_stub for ARM profile")
        }
    }
}

/// Core implementation of the IRQ_CONTROL syscall.
#[naked]
pub(crate) unsafe extern "C" fn sys_irq_control_stub(_mask: u32, _enable: u32) {
    cfg_if::cfg_if! {
        if #[cfg(armv6m)] {
            core::arch::asm!("
                @ Spill the registers we're about to use to pass stuff.
                push {{r4, r5, lr}}
                mov r4, r11
                push {{r4}}

                @ Load the constant syscall number.
                eors r4, r4
                adds r4, #{sysnum}
                mov r11, r4
                @ Move register arguments into place.
                mov r4, r0
                mov r5, r1

                @ To the kernel!
                svc #0

                @ This call returns no results.

                @ Restore the registers we used and return.
                pop {{r4}}
                mov r11, r4
                pop {{r4, r5, pc}}
                ",
                sysnum = const Sysnum::IrqControl as u32,
                options(noreturn),
            )
        } else if #[cfg(any(armv7m, armv8m))] {
            core::arch::asm!("
                @ Spill the registers we're about to use to pass stuff.
                push {{r4, r5, r11, lr}}

                @ Move register arguments into place.
                mov r4, r0
                mov r5, r1
                @ Load the constant syscall number.
                mov r11, {sysnum}

                @ To the kernel!
                svc #0

                @ This call returns no results.

                @ Restore the registers we used and return.
                pop {{r4, r5, r11, pc}}
                ",
                sysnum = const Sysnum::IrqControl as u32,
                options(noreturn),
            )
        } else {
            compile_error!("missing sys_irq_control stub for ARM profile")
        }
    }
}

/// Core implementation of the PANIC syscall.
#[naked]
pub(crate) unsafe extern "C" fn sys_panic_stub(
    _msg: *const u8,
    _len: usize,
) -> ! {
    cfg_if::cfg_if! {
        if #[cfg(armv6m)] {
            core::arch::asm!("
                @ We're not going to return, so technically speaking we don't
                @ need to save registers. However, we save them anyway, so that
                @ we can reconstruct the state that led to the panic.
                push {{r4, r5, lr}}
                mov r4, r11
                push {{r4}}

                @ Load the constant syscall number.
                eors r4, r4
                adds r4, #{sysnum}
                mov r11, r4
                @ Move register arguments into place.
                mov r4, r0
                mov r5, r1

                @ To the kernel!
                svc #0
                @ noreturn generates a udf to trap us if it returns.
                ",
                sysnum = const Sysnum::Panic as u32,
                options(noreturn),
            )
        } else if #[cfg(any(armv7m, armv8m))] {
            core::arch::asm!("
                @ We're not going to return, so technically speaking we don't
                @ need to save registers. However, we save them anyway, so that
                @ we can reconstruct the state that led to the panic.
                push {{r4, r5, r11, lr}}

                @ Move register arguments into place.
                mov r4, r0
                mov r5, r1
                @ Load the constant syscall number.
                mov r11, {sysnum}

                @ To the kernel!
                svc #0
                @ noreturn generates a udf to trap us if it returns.
                ",
                sysnum = const Sysnum::Panic as u32,
                options(noreturn),
            )
        } else {
            compile_error!("missing sys_panic_stub for ARM profile")
        }
    }
}

/// Core implementation of the GET_TIMER syscall.
#[naked]
pub(crate) unsafe extern "C" fn sys_get_timer_stub(_out: *mut RawTimerState) {
    cfg_if::cfg_if! {
        if #[cfg(armv6m)] {
            core::arch::asm!("
                @ Spill the registers we're about to use to pass stuff.
                push {{r4-r7, lr}}
                mov r4, r8
                mov r5, r9
                mov r6, r10
                mov r7, r11
                push {{r4-r7}}
                @ Load the constant syscall number.
                eors r4, r4
                adds r4, #{sysnum}
                mov r11, r4

                @ To the kernel!
                svc #0

                @ Write all the results out into the raw output buffer.
                stm r0!, {{r4-r7}}
                mov r4, r8
                mov r5, r9
                stm r0!, {{r4, r5}}
                @ Restore the registers we used.
                pop {{r4-r7}}
                mov r11, r7
                mov r10, r6
                mov r9, r5
                mov r8, r4
                pop {{r4-r7, pc}}
                ",
                sysnum = const Sysnum::GetTimer as u32,
                options(noreturn),
            )
        } else if #[cfg(any(armv7m, armv8m))] {
            core::arch::asm!("
                @ Spill the registers we're about to use to pass stuff.
                push {{r4-r11}}
                @ Load the constant syscall number.
                mov r11, {sysnum}

                @ To the kernel!
                svc #0

                @ Write all the results out into the raw output buffer.
                stm r0, {{r4-r9}}
                @ Restore the registers we used.
                pop {{r4-r11}}
                @ Fin.
                bx lr
                ",
                sysnum = const Sysnum::GetTimer as u32,
                options(noreturn),
            )
        } else {
            compile_error!("missing sys_get_timer_stub for ARM profile")
        }
    }
}

/// Core implementation of the POST syscall.
#[naked]
pub(crate) unsafe extern "C" fn sys_post_stub(_tid: u32, _mask: u32) -> u32 {
    cfg_if::cfg_if! {
        if #[cfg(armv6m)] {
            core::arch::asm!("
                @ Spill the registers we're about to use to pass stuff.
                push {{r4, r5, lr}}
                mov r4, r11
                push {{r4}}

                @ Load the constant syscall number.
                movs r4, #0
                adds r4, #{sysnum}
                mov r11, r4

                @ Move register arguments into place.
                mov r4, r0
                mov r5, r1

                @ To the kernel!
                svc #0

                @ Move result into place.
                mov r0, r4

                @ Restore the registers we used and return.
                pop {{r4}}
                mov r11, r4
                pop {{r4, r5, pc}}
                ",
                sysnum = const Sysnum::Post as u32,
                options(noreturn),
            )
        } else if #[cfg(any(armv7m, armv8m))] {
            core::arch::asm!("
                @ Spill the registers we're about to use to pass stuff.
                push {{r4, r5, r11, lr}}

                @ Move register arguments into place.
                mov r4, r0
                mov r5, r1
                @ Load the constant syscall number.
                mov r11, {sysnum}

                @ To the kernel!
                svc #0

                @ Move result into place.
                mov r0, r4

                @ Restore the registers we used and return.
                pop {{r4, r5, r11, pc}}
                ",
                sysnum = const Sysnum::Post as u32,
                options(noreturn),
            )
        } else {
            compile_error!("missing sys_post_stub for ARM profile")
        }
    }
}

/// Core implementation of the REPLY_FAULT syscall.
#[naked]
pub(crate) unsafe extern "C" fn sys_reply_fault_stub(_tid: u32, _reason: u32) {
    cfg_if::cfg_if! {
        if #[cfg(armv6m)] {
            core::arch::asm!("
                @ Spill the registers we're about to use to pass stuff.
                push {{r4, r5, lr}}
                mov r4, r11
                push {{r4}}

                @ Load the constant syscall number.
                movs r4, #0
                adds r4, #{sysnum}
                mov r11, r4
                @ Move register arguments into place.
                mov r4, r0
                mov r5, r1

                @ To the kernel!
                svc #0

                @ This syscall has no results.

                @ Restore the registers we used and return.
                pop {{r4}}
                mov r11, r4
                pop {{r4, r5, pc}}
                ",
                sysnum = const Sysnum::ReplyFault as u32,
                options(noreturn),
            )
        } else if #[cfg(any(armv7m, armv8m))] {
            core::arch::asm!("
                @ Spill the registers we're about to use to pass stuff.
                push {{r4, r5, r11, lr}}

                @ Move register arguments into place.
                mov r4, r0
                mov r5, r1
                @ Load the constant syscall number.
                mov r11, {sysnum}

                @ To the kernel!
                svc #0

                @ This syscall has no results.

                @ Restore the registers we used and return.
                pop {{r4, r5, r11, pc}}
                ",
                sysnum = const Sysnum::ReplyFault as u32,
                options(noreturn),
            )
        } else {
            compile_error!("missing sys_reply_fault_stub for ARM profile")
        }
    }
}
