#![no_std]

use riscv::register::mtvec::{self, TrapMode};

pub use riscv_rt::entry;

cfg_if::cfg_if! {
    if #[cfg(feature = "vectored-interrupts")] {
        // Create a trap table to vector interrupts to the correct handler.
        // NOTE: This MUST be aligned to at least a 4-byte boundary. Some
        //       targets have larger requirements, so we've gone with the
        //       highest so far: 256.
        // TODO: Currently all pass through common function, but could be
        //       vectored directly
        core::arch::global_asm! {"
            .section .trap, \"ax\"
            .globl _trap_table
            .align 8
            _trap_table:
            .rept 256 # TODO: This may need to be changed
            j _start_trap
            .endr
        "}
        extern "C" {
            static _trap_table: u32;
        }

        #[export_name = "_setup_interrupts"]
        extern "C" fn _setup_interrupts() {
            // SAFETY: the _trap_table has the right alignment
            unsafe {
                mtvec::write(
                    core::ptr::addr_of!(_trap_table) as usize,
                    TrapMode::Vectored,
                );
            };
        }
    } else {
        extern "C" {
            static _start_trap: u32;
        }
        extern "C" fn _setup_interrupts() {
            // SAFETY: the _trap_table has the right alignment
            unsafe {
                mtvec::write(
                    core::ptr::addr_of!(_start_trap) as usize,
                    TrapMode::Direct,
                );
            }
        }
    }
}
