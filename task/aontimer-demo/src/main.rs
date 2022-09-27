// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

#![no_std]
#![no_main]

use drv_ot_aontimer::*;
use drv_ext_int_ctrl_api::*;
use userlib::*;

task_slot!(INT_CONTROLLER, ext_int_ctrl);

task_config::task_config!{
    clock_frequency_hz: u32,
    bark_threshold_ticks: u64,
    bite_threshold_ticks: u64,
}

//TODO should be replaced by build time constants
// see jira https://rivosinc.atlassian.net/browse/SW-530
const AONTIMER_BARK_NOTIFICATION: u32 = 0x1;
const AONTIMER_TIME_NOTIFICATION: u32 = 0x2;

fn sleep_and_listen(timer: &AonTimer, int_ctrl: &ExtIntCtrl) {
    sys_log!("Sleeping...");
    let alarm = sys_get_timer().now;
    sys_set_timer(Some(alarm + 1000), AONTIMER_TIME_NOTIFICATION);
    loop {
        let result = sys_recv_closed(
            &mut [],
            AONTIMER_BARK_NOTIFICATION | AONTIMER_TIME_NOTIFICATION,
            TaskId::KERNEL,
        )
        .unwrap();
        int_ctrl.complete_int(AONTIMER_BARK_NOTIFICATION).unwrap();
        if result.operation & AONTIMER_BARK_NOTIFICATION != 0x0 {
            sys_log!("{}", sys_get_timer().now);
            //reset the interrupt
            if timer.bark_cb.is_some() {
                sys_log!("calling back");
                (timer.bark_cb.unwrap())();
            }
        }
        if result.operation & AONTIMER_TIME_NOTIFICATION != 0x0 {
            // Comment the following line of code to recieve the bark, and then be forced reset.
            return; //You've woken up
        }
    }
}

#[export_name = "main"]
fn main() -> ! {
    let int_ctrl = ExtIntCtrl::from(INT_CONTROLLER.get_task_id());

    //TODO change to const read from chip config
    //see jira https://rivosinc.atlassian.net/browse/SW-431
    const AONTIMER_BASE: u32 = 0x4047_0000;
    sys_log!("Restarted...");

    int_ctrl.enable_int(AONTIMER_BARK_NOTIFICATION).unwrap();

    let timer = AonTimer::new(
        AONTIMER_BASE,
        TASK_CONFIG.clock_frequency_hz,
        TASK_CONFIG.bark_threshold_ticks,
        TASK_CONFIG.bite_threshold_ticks,
        Some(|| {
            sys_log!("Bark!");
        }),
    ).unwrap();

    timer.enable().unwrap();
    loop {
        //Two things need to be queried:
        // 1. When the timer expires, we should feed the watchdog.
        // 2. When the bark interrupt is raised, handle it.
        sleep_and_listen(&timer, &int_ctrl);
        sys_log!("Feeding...");
        timer.feed_sacrifice();
    }
}
