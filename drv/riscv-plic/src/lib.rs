// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
#![no_std]

use drv_int_ctrl_api::{IntCtrlApi, IrqStatus, UsageError};
use tock_registers::interfaces::{Readable, Writeable};
use tock_registers::registers::ReadWrite;
use tock_registers::{register_bitfields, register_structs};

impl<const PRIO_BITS: u32, const MAX_IRQ: u32, const CTX: usize> IntCtrlApi
    for Plic<PRIO_BITS, MAX_IRQ, CTX>
{
    fn get_irq(&mut self) -> u32 {
        let irq = self.claim();
        irq as u32
    }

    fn get_irq_status(&self, irq: u32) -> Result<IrqStatus, UsageError> {
        if irq > MAX_IRQ {
            return Err(UsageError::NoIrq);
        }
        let mut status = IrqStatus::empty();
        if self.is_enabled(irq as usize) {
            status.set(IrqStatus::ENABLED, true);
        }
        if self.is_pending(irq as usize) {
            status.set(IrqStatus::PENDING, true);
        }
        Ok(status)
    }

    fn disable_irq(
        &mut self,
        irq: u32,
        also_clear_pending: bool,
    ) -> Result<(), UsageError> {
        if irq > MAX_IRQ {
            return Err(UsageError::NoIrq);
        }
        let irq = irq as usize;
        self.mask(irq);
        if also_clear_pending {
            self.complete(irq);
        }
        Ok(())
    }

    fn enable_irq(
        &mut self,
        irq: u32,
        also_clear_pending: bool,
    ) -> Result<(), UsageError> {
        if irq > MAX_IRQ {
            return Err(UsageError::NoIrq);
        }
        let irq = irq as usize;
        if also_clear_pending {
            self.complete(irq);
        }
        self.unmask(irq);
        Ok(())
    }
}

pub struct Plic<const PRIO_BITS: u32, const MAX_IRQ: u32, const CTX: usize> {
    regs: &'static PlicRegisters,
}

impl<const PRIO_BITS: u32, const MAX_IRQ: u32, const CTX: usize>
    Plic<PRIO_BITS, MAX_IRQ, CTX>
{
    const MAX_PRIO: u32 = (1 << PRIO_BITS) - 1;
    /// Create a new Plic
    /// # Safety
    /// Address must point to a PLIC device
    pub unsafe fn new(base_addr: usize) -> Self {
        let regs: &'static PlicRegisters =
            &mut *(base_addr as *mut PlicRegisters);

        Self { regs }
    }

    pub fn init(&self) {
        let mut regs_num = MAX_IRQ / 32;
        if MAX_IRQ % 32 > 0 {
            regs_num += 1;
        }
        // disable all interrupts
        for idx in 0..regs_num as usize {
            self.regs.iectx[CTX].enable[idx].set(0);
        }

        // init priorities
        for irq in 1..=MAX_IRQ as usize {
            self.set_priority(irq, 0);
        }
    }

    pub fn is_enabled(&self, irq: usize) -> bool {
        let enables = self.regs.iectx[CTX].enable[irq / 32].get();
        (enables & ((irq as u32) % 32)) != 0x0
    }

    /// Enable interrupt for context
    pub fn unmask(&self, irq: usize) {
        let v = self.regs.iectx[CTX].enable[irq / 32].get();
        self.regs.iectx[CTX].enable[irq / 32].set(v | (0x1 << (irq % 32)));
    }

    /// Disable interrupt for context
    pub fn mask(&self, irq: usize) {
        let v = self.regs.iectx[CTX].enable[irq / 32].get();
        self.regs.iectx[CTX].enable[irq / 32].set(v & !(0x1 << (irq % 32)));
    }

    /// Get interrupt priority
    pub fn get_priority(self, irq: usize) -> u32 {
        self.regs.priority[irq].get()
    }

    /// Set interrupt priority
    pub fn set_priority(&self, irq: usize, prio: u32) {
        let prio = if prio > Self::MAX_PRIO {
            Self::MAX_PRIO
        } else {
            prio
        };
        self.regs.priority[irq].set(prio);
    }

    /// Get threshold for context
    pub fn get_threshold(&self) -> u32 {
        self.regs.icctx[CTX].threshold.get()
    }

    /// Set threshold for context
    pub fn set_threshold(&self, threshold: u32) {
        let threshold = if threshold > Self::MAX_PRIO {
            Self::MAX_PRIO
        } else {
            threshold
        };
        self.regs.icctx[CTX].threshold.set(threshold);
    }

    /// Claim interrupt (used by interrupt runtime)
    pub fn claim(&self) -> usize {
        self.regs.icctx[CTX].cc.read(CLAIM_COMPLETE::CC) as usize
    }

    /// Complete interrupt (used by interrupt runtime)
    pub fn complete(&self, irq: usize) {
        self.regs.icctx[CTX]
            .cc
            .write(CLAIM_COMPLETE::CC.val(irq as u32));
    }

    /// Checks if interrupt is pending
    pub fn is_pending(&self, irq: usize) -> bool {
        (self.regs.pending[irq / 32].get()) & (0x1 << (irq % 32)) != 0x0
    }
}

register_structs! {
    pub PlicIeCtx {
        /// Interrupt Enable Context
        (0x0000 => enable: [ReadWrite<u32>; 32]),
        (0x0080 => @END),
    },

    pub PlicIcCtx {
        /// Interrupt Threshold/Claim Complete Context
        (0x0000 => threshold: ReadWrite<u32>),
        (0x0004 => cc: ReadWrite<u32, CLAIM_COMPLETE::Register>),
        (0x0008 => _reserved0),
        (0x1000 => @END),
    },

    pub PlicRegisters {
        /// Interrupt Priority Registers
        (0x0000_0000 => priority: [ReadWrite<u32>; 1024]),
        /// Interrupt Pending Register
        (0x0000_1000 => pending: [ReadWrite<u32>; 32]),
        (0x0000_1080 => _reserved1),
        /// Interrupt Enable Context
        (0x0000_2000 => iectx: [PlicIeCtx; 15872]),
        (0x001F_2000 => _reserved2),
        /// Interrupt Threshold/Claim Complete Context
        (0x0020_0000 => icctx: [PlicIcCtx; 15872]),
        // msip.
        (0x0400_0000 => msip: [ReadWrite<u32, MSIP::Register>; 4096]),
        // Alert Test Register.
        (0x0400_4000 => alert_test: ReadWrite<u32, ALERT_TEST::Register>),
        (0x0400_4004 => @END),
    }
}

register_bitfields![u32,
    CLAIM_COMPLETE [
        CC OFFSET(0) NUMBITS(8) [],
    ],
    MSIP [
        MSIP OFFSET(0) NUMBITS(1) [],
    ],
    ALERT_TEST [
        FATAL_FAULT OFFSET(0) NUMBITS(1) [],
    ],
];
