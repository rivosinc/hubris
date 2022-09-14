// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

#![no_std]
#![no_main]

// NOTE: you will probably want to remove this when you write your actual code;
// we need to import userlib to get this to compile, but it throws a warning
// because we're not actually using it yet!
use userlib::*;
use drv_riscv_plic_api::*;
use aontimer as aon;

task_slot!(INT_CONTROLLER, ext_int_ctrl);

const AONTIMER_BARK: u32 = 0x1;
const AONTIMER_TIME: u32 = 0x2;

const BARK_INT: u32 = 0;

fn sleep_and_listen(timer: &aon::AonTimer) {
    sys_log!("Sleeping...");
    let alarm = sys_get_timer().now;
    sys_set_timer(Some(alarm + 1000), AONTIMER_TIME);
    loop {
        let result = sys_recv_closed(&mut [], AONTIMER_BARK | AONTIMER_TIME, TaskId::KERNEL).unwrap();
        if result.operation & AONTIMER_BARK != 0x0 {
            sys_log!("Bark!");
            sys_log!("{}",sys_get_timer().now);
            //reset the interrupt
            // timer.clear_wdt_irq();
            // sys_irq_control(AONTIMER_BARK, true);
            if timer.bark_cb.is_some() {
                sys_log!("calling back");
                (timer.bark_cb.unwrap())();
            }
        }
        if result.operation & AONTIMER_TIME != 0x0 {
            // Comment the following line of code to recieve the bark, and then be forced reset.
            return; //You've woken up
        }
    }
}

#[export_name = "main"]
fn main() -> ! {
    let int_ctrl = RiscvIntCtrl::from(INT_CONTROLLER.get_task_id());
    
    int_ctrl.disable_int(BARK_INT).unwrap();

    const AONTIMER_BASE: u32 = 0x4047_0000;
    const AONTIMER_FREQ: u32 = 200_000;
    sys_log!("Restarted...");

    int_ctrl.enable_int(BARK_INT).unwrap();

    // TODO this callback doesn't actually get called.
    let timer = aon::AonTimer::new(AONTIMER_BASE, AONTIMER_FREQ, 2000, 5000, Some(|| {sys_log!("Bark!");}));
    timer.enable();
    loop {
        //Two things need to be queried:
        // 1. When the timer expires, we should feed the watchdog.
        // 2. When the bark interrupt is raised, handle it.
        sleep_and_listen(&timer);
        sys_log!("Feeding...");
        timer.feed_sacrifice();
    }
}
