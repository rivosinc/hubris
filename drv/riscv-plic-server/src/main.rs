// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

#![no_std]
#![no_main]

use riscv::plic;
use riscv::plic::Plic;
use userlib::*;

use core::result::Result;

use drv_ext_int_ctrl_api::ExtIntCtrlError;
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

impl idl::InOrderExtIntCtrlImpl for ServerImpl {
    /// Disables the selected interrupt on the PLIC.
    fn disable_int(
        &mut self,
        msg: &userlib::RecvMessage,
        irq: u32,
    ) -> Result<(), RequestError<ExtIntCtrlError>> {
        let owner: InterruptOwner = InterruptOwner {
            task: msg.sender.index() as u32,
            notification: irq,
        };

        match get_task_irqs(owner) {
            Ok(irqs) => {
                let plic = unsafe { &mut *PLIC_REGISTER_BLOCK };
                for irq in irqs.iter() {
                    plic.mask(0, irq.0.try_into().unwrap());
                }

                return Ok(());
            }
            Err(()) => return Err(Runtime(ExtIntCtrlError::IRQUnassigned)),
        }
    }

    /// Enables the selected interrupt on the PLIC.
    fn enable_int(
        &mut self,
        msg: &userlib::RecvMessage,
        irq: u32,
    ) -> Result<(), RequestError<ExtIntCtrlError>> {
        let owner: InterruptOwner = InterruptOwner {
            task: msg.sender.index() as u32,
            notification: irq,
        };

        match get_task_irqs(owner) {
            Ok(irqs) => {
                let plic = unsafe { &mut *PLIC_REGISTER_BLOCK };
                for irq in irqs.iter() {
                    plic.unmask(0, irq.0.try_into().unwrap());
                }

                return Ok(());
            }
            Err(()) => return Err(Runtime(ExtIntCtrlError::IRQUnassigned)),
        }
    }

    /// Completes the interrupt on the PLIC, allowing for a new one to come
    /// through.
    fn complete_int(
        &mut self,
        msg: &userlib::RecvMessage,
        irq: u32,
    ) -> Result<(), RequestError<ExtIntCtrlError>> {
        let owner: InterruptOwner = InterruptOwner {
            task: msg.sender.index() as u32,
            notification: irq,
        };

        match get_task_irqs(owner) {
            Ok(irqs) => {
                let plic = unsafe { &mut *PLIC_REGISTER_BLOCK };
                for irq in irqs.iter() {
                    plic.complete(0, irq.0.try_into().unwrap());
                }

                return Ok(());
            }
            Err(()) => return Err(Runtime(ExtIntCtrlError::IRQUnassigned)),
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

            // An error means an interrupt came in on a line that no task has
            // ownership over.
            let owner: (TaskId, u32) = match get_irq_owner(irq) {
                Ok(owner) => owner,
                Err(()) => continue,
            };

            let code = sys_post(owner.0, owner.1);

            // The task that owns the line was restarted.
            if code & FIRST_DEAD_CODE == FIRST_DEAD_CODE {
                let new_task_id = TaskId::for_index_and_gen(
                    owner.0 .0.into(),
                    ((code & !FIRST_DEAD_CODE) as u8).into(),
                );

                // SAFETY: We already have the irq owner, so we know that this
                // operation will succeed. No need to bother checking for errors.
                unsafe {
                    set_irq_owner(irq, (new_task_id, owner.1))
                        .unwrap_unchecked();
                };
                sys_post(new_task_id, owner.1);
            }
        }

        sys_irq_control(PLIC_IRQ, true);
    }
}

#[export_name = "main"]
fn main() -> ! {
    let plic = unsafe { &mut *PLIC_REGISTER_BLOCK };
    plic.set_threshold(0, Priority::highest());

    // Set priority for interrupts that are used to a nonzero value. Used
    // interrupts are left masked as the task that owns them should decide when
    // they should first be enabled.
    for i in 1..1024 {
        let priority = if irq_assigned(i) {
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
    use drv_ext_int_ctrl_api::ExtIntCtrlError;
    include!(concat!(env!("OUT_DIR"), "/server_stub.rs"));
}

include!(concat!(env!("OUT_DIR"), "/plic_config.rs"));
