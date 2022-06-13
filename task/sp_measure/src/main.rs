// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

#![no_std]
#![no_main]

use drv_sp_ctrl_api::*;
use ringbuf::*;
use sha3::{Digest, Sha3_256};
use userlib::*;

const READ_SIZE: usize = 256;

const TRANSACTION_SIZE: u32 = 1024;

task_slot!(SP_CTRL, swd);

#[derive(Copy, Clone, PartialEq)]
enum Trace {
    Start(u64),
    End(u64),
    ShaGood,
    ShaBad,
    None,
}

ringbuf!(Trace, 16, Trace::None);

#[export_name = "main"]
fn main() -> ! {
    loop {
        let mut sha = Sha3_256::new();
        let sp_ctrl = SpCtrl::from(SP_CTRL.get_task_id());

        match sp_ctrl.setup() {
            Err(_) => panic!(),
            _ => (),
        }

        let mut data: [u8; READ_SIZE] = [0; READ_SIZE];

        let start = sys_get_timer().now;
        ringbuf_entry!(Trace::Start(start));
        for addr in (FLASH_START..FLASH_END).step_by(READ_SIZE) {
            if addr % TRANSACTION_SIZE == 0 {
                match sp_ctrl
                    .read_transaction_start(addr, addr + TRANSACTION_SIZE)
                {
                    Err(_) => panic!(),
                    Ok(_) => (),
                }
            }

            data.fill(0);
            match sp_ctrl.read_transaction(&mut data) {
                Err(_) => panic!(),
                Ok(_) => (),
            }

            sha.update(&data);
        }

        let sha_out = sha.finalize();

        let end = sys_get_timer().now;
        ringbuf_entry!(Trace::End(end));
        if sha_out.as_slice() == EXPECTED.as_slice() {
            ringbuf_entry!(Trace::ShaGood);
        } else {
            ringbuf_entry!(Trace::ShaBad);
        }

        // Wait for a notification that will never come, politer than
        // busy looping forever
        if sys_recv_closed(&mut [], 1, TaskId::KERNEL).is_err() {
            panic!();
        }
    }
}

include!(concat!(env!("OUT_DIR"), "/expected.rs"));
