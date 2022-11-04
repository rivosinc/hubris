// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! User application architecture support stubs for RISC-V
//!
//! See the note on syscall stubs at the top of the userlib module for
//! rationale.

use crate::*;
use core::arch::asm;

/// This is the entry point for the kernel. Its job is to set up our memory
/// before jumping to the `userlib_main` stub.
#[doc(hidden)]
#[no_mangle]
#[link_section = ".text.start"]
#[naked]
pub unsafe extern "C" fn _start() -> ! {
    asm!("
        # Copy data initialization image into data section.
        la t0, __edata       # upper bound in t0
        la t1, __sidata      # source in t1
        la t2, __sdata       # dest in t2
        j 1f

    2:  ld s3, (t1)
        add t1, t1, 8
        sd s3, (t2)
        add t2, t2, 8

    1:  bne t2, t0, 2b

        # Zero BSS
        la t0, __ebss        # upper bound in t0
        la t1, __sbss        # base in t1
        j 1f

    2:  sd zero, (t1)
        add t1, t1, 8

    1:  bne t1, t0, 2b
        j {userlib_main}
        ",
        userlib_main = sym userlib_main,
        options(noreturn),
    )
}

/// Core implementation of the REFRESH_TASK_ID syscall.
#[naked]
pub(crate) unsafe extern "C" fn sys_refresh_task_id_stub(_tid: u32) -> u32 {
    asm!("
        # Load the constant syscall number.
        li a7, {sysnum}

        # To the kernel!
        ecall

        # Results are placed into the correct registers by the kernel, we can
        # just return now.
        ret
        ",
        sysnum = const Sysnum::RefreshTaskId as u32,
        options(noreturn),
    )
}

/// Core implementation of the SEND syscall.
#[naked]
pub(crate) unsafe extern "C" fn sys_send_stub(
    _args: &mut SendArgs<'_>,
) -> RcLen {
    asm!("
        # Load in args from the struct (SendArgs).
        ld a6, 5*8(a0)    # lease_len
        ld a5, 4*8(a0)    # lease_ptr
        ld a4, 3*8(a0)    # incoming_len
        ld a3, 2*8(a0)    # incoming_ptr
        ld a2, 1*8(a0)    # outgoing_len
        ld a1, 0*8(a0)    # outgoing_ptr
        lw a0, 6*8(a0)    # packed_target_operation

        # Load the constant syscall number.
        li a7, {sysnum}

        # To the kernel!
        ecall

        slli a1, a1, 0x20
        or a0, a0, a1

        ret
        ",
        sysnum = const Sysnum::Send as u32,
        options(noreturn),
    )
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
    asm!("
        # Preserve output buffer pointer in callee-save register, ensuring
        # it is saved on the stack, which is kept 16-byte aligned.
        addi sp, sp, -8*8
        sd s2, 0*8(sp)
        mv s2, a4

        # Load the constant syscall number.
        li a7, {sysnum}

        # To the kernel!
        ecall

        # Write all the results out into the raw output buffer (RawRecvMessage).
        sw a1, 0(s2)     # sender
        sw a2, 4(s2)     # operation
        sd a3, 8(s2)     # message_len
        sd a4, 16(s2)    # response_capacity
        sd a5, 24(s2)    # lease_count

        # Restore callee-save register and stack pointer and return.
        ld s2, 0*8(sp)
        addi sp, sp, 8*8
        ret
        ",
        sysnum = const Sysnum::Recv as u32,
        options(noreturn),
    )
}

/// Core implementation of the REPLY syscall.
#[naked]
pub(crate) unsafe extern "C" fn sys_reply_stub(
    _peer: u32,
    _code: u32,
    _message_ptr: *const u8,
    _message_len: usize,
) {
    asm!("
        # Load the constant syscall number.
        li a7, {sysnum}

        # To the kernel!
        ecall

        # No results, we can just return now.
        ret
        ",
        sysnum = const Sysnum::Reply as u32,
        options(noreturn),
    )
}

/// Core implementation of the SET_TIMER syscall.
#[naked]
pub(crate) unsafe extern "C" fn sys_set_timer_stub(
    _set_timer: u32,
    _deadline_lo: u32,
    _deadline_hi: u32,
    _notification: u32,
) {
    asm!("
        # Load the constant syscall number.
        li a7, {sysnum}

        # To the kernel!
        ecall

        # No results, we can just return now.
        ret
        ",
        sysnum = const Sysnum::SetTimer as u32,
        options(noreturn),
    )
}

/// Core implementation of the BORROW_READ syscall.
#[naked]
pub(crate) unsafe extern "C" fn sys_borrow_read_stub(
    _args: *mut BorrowReadArgs,
) -> RcLen {
    asm!("
        # Move register arguments into place, in reverse order so that a0 is
        # loaded last when we're finished with it.
        # (BorrowReadArgs)
        ld a4, 24(a0)     # dest_len
        ld a3, 16(a0)     # dest
        ld a2, 8(a0)      # offset
        ld a1, 0(a0)      # index
        lw a0, 32(a0)     # lender

        # Load the constant syscall number.
        li a7, {sysnum}

        # To the kernel!
        ecall

        slli a1, a1, 0x20
        or a0, a0, a1

        ret
        ",
        sysnum = const Sysnum::BorrowRead as u32,
        options(noreturn),
    )
}

/// Core implementation of the BORROW_WRITE syscall.
#[naked]
pub(crate) unsafe extern "C" fn sys_borrow_write_stub(
    _args: *mut BorrowWriteArgs,
) -> RcLen {
    asm!("
        # Move register arguments into place, in reverse order so that a0 is
        # loaded last when we're finished with it.
        # (BorrowWriteArgs)
        ld a4, 24(a0)      # src_len
        ld a3, 16(a0)      # src
        ld a2, 8(a0)       # offset
        ld a1, 0(a0)       # index
        lw a0, 32(a0)      # lender

        # Load the constant syscall number.
        li a7, {sysnum}

        # To the kernel!
        ecall

        slli a1, a1, 0x20
        or a0, a0, a1

        ret
        ",
        sysnum = const Sysnum::BorrowWrite as u32,
        options(noreturn),
    )
}

/// Core implementation of the BORROW_INFO syscall.
#[naked]
pub(crate) unsafe extern "C" fn sys_borrow_info_stub(
    _lender: u32,
    _index: usize,
    _out: *mut RawBorrowInfo,
) {
    asm!("
        # Preserve output buffer pointer in callee-save register, ensuring
        # it is saved on the stack, which is kept 16-byte aligned.
        addi sp, sp, -8*8
        sd s2, 0*8(sp)
        mv s2, a2

        # Load the constant syscall number.
        li a7, {sysnum}

        # To the kernel!
        ecall

        # Write all the results out into the raw output buffer (RawBorrowInfo).
        sw a0, 0(s2)    # rc
        sw a1, 4(s2)    # atts
        sd a2, 8(s2)    # length

        # Restore callee-save register and stack pointer and return.
        ld s2, 0*8(sp)
        addi sp, sp, 8*8
        ret
        ",
        sysnum = const Sysnum::BorrowInfo as u32,
        options(noreturn),
    )
}

/// Core implementation of the IRQ_CONTROL syscall.
#[naked]
pub(crate) unsafe extern "C" fn sys_irq_control_stub(_mask: u32, _enable: u32) {
    asm!("
        # Load the constant syscall number.
        li a7, {sysnum}

        # To the kernel!
        ecall

        # No results, we can just return now.
        ret
        ",
        sysnum = const Sysnum::IrqControl as u32,
        options(noreturn),
    )
}

/// Core implementation of the PANIC syscall.
#[naked]
pub(crate) unsafe extern "C" fn sys_panic_stub(
    _msg: *const u8,
    _len: usize,
) -> ! {
    asm!("
        # Load the constant syscall number.
        li a7, {sysnum}

        # To the kernel!
        ecall

        # This really shouldn't return. Ensure this:
        unimp
        ",
        sysnum = const Sysnum::Panic as u32,
        options(noreturn),
    )
}

/// Core implementation of the GET_TIMER syscall.
#[naked]
pub(crate) unsafe extern "C" fn sys_get_timer_stub(_out: *mut RawTimerState) {
    asm!("
        # Preserve output buffer pointer in callee-save register, ensuring
        # it is saved on the stack, which is kept 16-byte aligned.
        addi sp, sp, -8*8
        sd s2, 0*8(sp)
        mv s2, a0

        # Load the constant syscall number.
        li a7, {sysnum}

        # To the kernel!
        ecall

        # Write all the results out into the raw output buffer (RawTimerState).

        sw a0, 0*4(s2)      # now_lo
        sw a1, 1*4(s2)      # now_hi
        sw a2, 2*4(s2)      # set
        sw a3, 3*4(s2)      # dl_lo
        sw a4, 4*4(s2)      # dl_hi
        sw a5, 5*4(s2)      # on_dl

        # Restore callee-save register and stack pointer and return.
        ld s2, 0*8(sp)
        addi sp, sp, 8*8
        ret
        ",
        sysnum = const Sysnum::GetTimer as u32,
        options(noreturn),
    )
}

/// Core implementation of the POST syscall.
#[naked]
pub(crate) unsafe extern "C" fn sys_post_stub(_tid: u32, _mask: u32) -> u32 {
    asm!("
        # Load the constant syscall number.
        li a7, {sysnum}

        # To the kernel!
        ecall

        # Results are placed into the correct registers by the kernel, we can
        # just return now.
        ret
        ",
        sysnum = const Sysnum::Post as u32,
        options(noreturn),
    )
}

/// Core implementation of the REPLY_FAULT syscall.
#[naked]
pub(crate) unsafe extern "C" fn sys_reply_fault_stub(_tid: u32, _reason: u32) {
    asm!("
        # Load the constant syscall number.
        li a7, {sysnum}

        # To the kernel!
        ecall

        # No results, we can just return now.
        ret
        ",
        sysnum = const Sysnum::ReplyFault as u32,
        options(noreturn),
    )
}
