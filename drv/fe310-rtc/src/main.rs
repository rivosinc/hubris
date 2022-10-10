// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

#![no_std]
#![no_main]

use drv_ext_int_ctrl_api::*;
use ringbuf::*;
#[allow(unused_imports)]
use userlib::*;

task_slot!(INT_CONTROLLER, ext_int_ctrl);

#[export_name = "main"]
fn main() -> ! {
    ringbuf!(u64, 64, 0);

    let mut num_ints: u64 = 0;

    let int_ctrl: ExtIntCtrl = ExtIntCtrl::from(INT_CONTROLLER.get_task_id());

    // The PLIC should disable all interrupts on reset, but this is here just to
    // be sure, as well as guaranteee it's disabled to verify that the enable
    // works.
    int_ctrl.disable_int(RTC_NOTIFICATION).unwrap();

    let regs =
        core::ptr::slice_from_raw_parts_mut(AON_RTC_BASE_ADDR as *mut u32, 10);
    unsafe {
        (*regs)[2] = 0x0;
        (*regs)[3] = 0x0;
        (*regs)[8] = 0x1 << 2;
        (*regs)[0] = (0x1 << 12) | (0xF);
    };

    int_ctrl.enable_int(RTC_NOTIFICATION).unwrap();

    loop {
        let result =
            sys_recv_closed(&mut [], RTC_NOTIFICATION, TaskId::KERNEL).unwrap();

        if result.operation & RTC_NOTIFICATION != 0x0 {
            unsafe {
                (*regs)[2] = 0x0;
                (*regs)[3] = 0x0;
            };
            num_ints += 1;

            sys_log!("RTC Interrupt number {}", num_ints);
            ringbuf_entry!(num_ints);

            int_ctrl.complete_int(RTC_NOTIFICATION).unwrap();
        }
    }
}

include!(concat!(env!("OUT_DIR"), "/rtc_config.rs"));
