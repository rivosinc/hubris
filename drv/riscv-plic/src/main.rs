// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

#![no_std]
#![no_main]

use riscv::plic;
use riscv::plic::Plic;
use userlib::*;

use core::result::Result;

use drv_riscv_plic_api::RiscvIntCtrlError;
use idol_runtime::RequestError;
use idol_runtime::RequestError::Runtime;

const PLIC_IRQ: u32 = 0x0000_0001;

fn get_irq_owner(irq: u32) -> Result<(TaskId, u32), ()> {
    return match unsafe { HUBRIS_IRQ_TASK_LOOKUP.get(InterruptNum(irq)) } {
        Some(task) => return Ok(*task),
        None => Err(()),
    };
}

fn set_irq_owner(irq: u32, owner: (TaskId, u32)) -> Result<(), ()> {
    return unsafe { HUBRIS_IRQ_TASK_LOOKUP.set(InterruptNum(irq), owner) };
}

fn get_task_irqs(
    owner: InterruptOwner,
) -> Result<&'static [userlib::InterruptNum], ()> {
    return match HUBRIS_TASK_IRQ_LOOKUP.get(owner) {
        Some(irqs) => Ok(irqs),
        None => Err(()),
    };
}

fn irq_assigned(irq: u32) -> bool {
    return unsafe { HUBRIS_IRQ_TASK_LOOKUP.contains(InterruptNum(irq)) };
}

#[repr(C)]
struct ServerImpl {}

impl idl::InOrderRiscvIntCtrlImpl for ServerImpl {
    /// Disables the selected interrupt on the PLIC.
    fn disable_int(
        &mut self,
        msg: &userlib::RecvMessage,
        irq: u32,
    ) -> Result<(), RequestError<RiscvIntCtrlError>> {
        let owner: InterruptOwner = InterruptOwner {
            task: msg.sender.index() as u32,
            notification: irq,
        };

        match get_task_irqs(owner) {
            Ok(irqs) => {
                let plic = unsafe { &mut *PLIC_REGISTER_BLOCK };
                plic.mask(0, irqs[0].0.try_into().unwrap());

                return Ok(());
            }
            Err(()) => return Err(Runtime(RiscvIntCtrlError::IRQUnassigned)),
        }
    }

    /// Enables the selected interrupt on the PLIC.
    fn enable_int(
        &mut self,
        msg: &userlib::RecvMessage,
        irq: u32,
    ) -> Result<(), RequestError<RiscvIntCtrlError>> {
        let owner: InterruptOwner = InterruptOwner {
            task: msg.sender.index() as u32,
            notification: irq,
        };

        match get_task_irqs(owner) {
            Ok(irqs) => {
                let plic = unsafe { &mut *PLIC_REGISTER_BLOCK };
                plic.unmask(0, irqs[0].0.try_into().unwrap());

                return Ok(());
            }
            Err(()) => return Err(Runtime(RiscvIntCtrlError::IRQUnassigned)),
        }
    }

    /// Completes the interrupt on the PLIC, allowing for a new one to come
    /// through.
    fn complete_int(
        &mut self,
        msg: &userlib::RecvMessage,
        irq: u32,
    ) -> Result<(), RequestError<RiscvIntCtrlError>> {
        let owner: InterruptOwner = InterruptOwner {
            task: msg.sender.index() as u32,
            notification: irq,
        };

        match get_task_irqs(owner) {
            Ok(irqs) => {
                let plic = unsafe { &mut *PLIC_REGISTER_BLOCK };
                plic.complete(0, irqs[0].0.try_into().unwrap());

                return Ok(());
            }
            Err(()) => return Err(Runtime(RiscvIntCtrlError::IRQUnassigned)),
        }
    }
}

impl idol_runtime::NotificationHandler for ServerImpl {
    // The PLIC is only interested in interrupt notifications from the kernel
    fn current_notification_mask(&self) -> u32 {
        return PLIC_IRQ;
    }

    // An interrupt has come in.
    // NOTE: Currently, the driver assumes that all interrupts come in on
    //       Context 0.
    fn handle_notification(&mut self, _bits: u32) {
        let plic = unsafe { &mut *PLIC_REGISTER_BLOCK };
        loop {
            let irq: u32 = match plic.claim(0) {
                Some(irq) => core::primitive::u16::from(irq) as u32,
                None => break,
            };

            let owner: (TaskId, u32) = match get_irq_owner(irq) {
                Ok(owner) => owner,
                Err(()) => break,
            };

            let code = sys_post(owner.0, owner.1);

            if code & 0xFFFF_FF00 == 0xFFFF_FF00 {
                let new_task_id = TaskId::for_index_and_gen(
                    owner.0 .0.into(),
                    ((code & 0x0000_00FF) as u8).into(),
                );

                match set_irq_owner(irq, (new_task_id, owner.1)) {
                    Ok(()) => sys_post(new_task_id, owner.1),
                    Err(()) => break,
                };
            }
        }

        sys_irq_control(PLIC_IRQ, true);
    }
}

#[export_name = "main"]
fn main() -> ! {
    let plic = unsafe { &mut *PLIC_REGISTER_BLOCK };
    plic.set_threshold(0, Priority::highest());

    // Zero all interrupt sources ...
    for i in 1..1024 {
        let priority = if irq_assigned(i) {
            plic.unmask(0, i as usize);
            Priority::from_bits(1)
        } else {
            Priority::never()
        };

        plic.set_priority(i as usize, priority);
    }

    let mut incoming = [0u8; idl::INCOMING_SIZE];

    plic.set_threshold(0, Priority::never());
    let mut server: ServerImpl = ServerImpl {};
    sys_irq_control(PLIC_IRQ, true);
    loop {
        idol_runtime::dispatch_n(&mut incoming, &mut server);
    }
}

// And the Idol bits
mod idl {
    use drv_riscv_plic_api::RiscvIntCtrlError;
    include!(concat!(env!("OUT_DIR"), "/server_stub.rs"));
}

include!(concat!(env!("OUT_DIR"), "/plic_config.rs"));
