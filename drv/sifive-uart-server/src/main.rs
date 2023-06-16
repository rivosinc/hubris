// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

#![no_std]
#![no_main]

use drv_sifive_uart::SifiveUart;
use drv_usart_api::UsartApiError;
use idol_runtime::{
    ClientError, Leased, LenLimit, NotificationHandler,
    RequestError, R,
};
use userlib::*;

task_config::task_config! {
    clk_freq_hz: u64,
    baud_rate: u64,
}

const STR_MAX_SIZE: usize = 256;

#[export_name = "main"]
fn main() -> ! {
    if let Ok(mut server) = unsafe { ServerImpl::new(UART_BASE_ADDR) } {
        let mut incoming = [0u8; INCOMING_SIZE];

        loop {
            idol_runtime::dispatch(&mut incoming, &mut server);
        }
    } else {
        loop {
            while sys_get_timer().now > 0 {}
        }
    }
}

struct ServerImpl {
    uart: SifiveUart,
    buffer: [u8; STR_MAX_SIZE],
}

impl ServerImpl {
    pub unsafe fn new(base_addr: u32) -> Result<Self, ()> {
        let mut uart =
            // We are just going to unwrap the result because there is nothing this task can do
            // if the uart can't be initialized
            unsafe { SifiveUart::new(base_addr) };

        // setup uart baudrate
        uart.set_baud_rate(TASK_CONFIG.clk_freq_hz, TASK_CONFIG.baud_rate);

        // enable uart transmit
        uart.tx_enable(true);

        let buffer = [0u8; STR_MAX_SIZE];

        Ok(Self { uart, buffer })
    }
}

impl InOrderUsartApiImpl for ServerImpl {
    fn write(
        &mut self,
        _msg: &userlib::RecvMessage,
        data: LenLimit<Leased<R, [u8]>, STR_MAX_SIZE>,
    ) -> Result<(), idol_runtime::RequestError<UsartApiError>> {
        let len = data.len();

        if len > STR_MAX_SIZE {
            return Err(RequestError::Runtime(UsartApiError::BadTransferSize));
        }
        data
        .read_range(0..len, &mut self.buffer)
        .map_err(|_| RequestError::Fail(ClientError::WentAway))?;

        self.uart.write_buf(&self.buffer[..len]);

        Ok(())
    }
}

impl NotificationHandler for ServerImpl {
    fn current_notification_mask(&self) -> u32 {
        // We don't use notifications, don't listen for any.
        0
    }

    fn handle_notification(&mut self, _bits: u32) {
        unreachable!()
    }
}

include!(concat!(env!("OUT_DIR"), "/server_stub.rs"));
include!(concat!(env!("OUT_DIR"), "/notifications.rs"));
include!(concat!(env!("OUT_DIR"), "/peripheral_config.rs"));
