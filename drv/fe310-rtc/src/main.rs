// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

#![no_std]
#![no_main]

use ringbuf::*;
#[allow(unused_imports)]
use userlib::*;

#[export_name = "main"]
fn main() -> ! {
    const RTC_INT: u32 = 0x1;
    ringbuf!(u64, 64, 0);

    sys_irq_control(RTC_INT, true);

    let mut num_ints: u64 = 0;

    let regs = core::ptr::slice_from_raw_parts_mut(0x1000_0040 as *mut u32, 10);
    unsafe {
        (*regs)[2] = 0x0;
        (*regs)[3] = 0x0;
        (*regs)[8] = 0x1 << 2;
        (*regs)[0] = (0x1 << 12) | (0xF);
    };

    loop {
        let result = sys_recv_closed(&mut [], RTC_INT, TaskId::KERNEL).unwrap();

        if result.operation & RTC_INT != 0x0 {
            unsafe {
                (*regs)[2] = 0x0;
                (*regs)[3] = 0x0;
            };
            num_ints += 1;

            sys_log!("Recieved RTC Interrupt number {}", num_ints);
            ringbuf_entry!(num_ints);

            sys_irq_control(RTC_INT, true);
        }
    }
}
