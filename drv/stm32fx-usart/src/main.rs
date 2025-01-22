// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! A driver for the STM32F4 U(S)ART.
//!
//! # IPC protocol
//!
//! ## `write` (1)
//!
//! Sends the contents of lease #0. Returns when completed.

#![no_std]
#![no_main]

use drv_usart_api::UsartApiError;
use idol_runtime::{
    ClientError, Leased, LenLimit, NotificationHandler,
    RequestError, R,
};

#[cfg(feature = "stm32f4")]
use stm32f4::stm32f407 as device;

#[cfg(feature = "stm32f3")]
use stm32f3::stm32f303 as device;

use userlib::*;
use zerocopy::AsBytes;

task_slot!(RCC, rcc_driver);

const STR_MAX_SIZE: usize = 256;

struct Transmit {
    buffer: [u8; STR_MAX_SIZE],
    len: usize,
    pos: usize,
}

#[export_name = "main"]
fn main() -> ! {
    if let Ok(mut server) = unsafe { ServerImpl::new() } {
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

fn turn_on_usart() {
    let rcc_driver = RCC.get_task_id();

    const ENABLE_CLOCK: u16 = 1;
    let pnum = 113; // see bits in APB1ENR
    let (code, _) = userlib::sys_send(
        rcc_driver,
        ENABLE_CLOCK,
        pnum.as_bytes(),
        &mut [],
        &[],
    );
    assert_eq!(code, 0);

    const LEAVE_RESET: u16 = 4;
    let (code, _) = userlib::sys_send(
        rcc_driver,
        LEAVE_RESET,
        pnum.as_bytes(),
        &mut [],
        &[],
    );
    assert_eq!(code, 0);
}

fn turn_on_gpioa() {
    let rcc_driver = RCC.get_task_id();

    const ENABLE_CLOCK: u16 = 1;

    #[cfg(feature = "stm32f3")]
    let pnum = 17; // see bits in AHBENR
    #[cfg(feature = "stm32f4")]
    let pnum = 0; // see bits in AHB1ENR

    let (code, _) = userlib::sys_send(
        rcc_driver,
        ENABLE_CLOCK,
        pnum.as_bytes(),
        &mut [],
        &[],
    );
    assert_eq!(code, 0);

    const LEAVE_RESET: u16 = 4;
    let (code, _) = userlib::sys_send(
        rcc_driver,
        LEAVE_RESET,
        pnum.as_bytes(),
        &mut [],
        &[],
    );
    assert_eq!(code, 0);
}

fn step_transmit(
    usart: &device::usart1::RegisterBlock,
    tx: &mut Transmit,
) {
    // Stuff byte into transmitter.
    let byte = tx.buffer[tx.pos];

    #[cfg(feature = "stm32f3")]
    usart.tdr.write(|w| w.tdr().bits(u16::from(byte)));
    #[cfg(feature = "stm32f4")]
    usart.dr.write(|w| w.dr().bits(u16::from(byte)));

    tx.pos += 1;
    if tx.pos == tx.len {
        tx.len = 0;
        usart.cr1.modify(|_, w| w.txeie().disabled());
    }
}


struct ServerImpl {
    usart: &'static device::usart1::RegisterBlock,
    // Field messages.
     txref: Transmit,
}

impl ServerImpl {
    pub unsafe fn new() -> Result<Self, ()> {
        // Turn the actual peripheral on so that we can interact with it.
        turn_on_usart();

        // From thin air, pluck a pointer to the USART register block.
        //
        // Safety: this is needlessly unsafe in the API. The USART is essentially a
        // static, and we access it through a & reference so aliasing is not a
        // concern. Were it literally a static, we could just reference it.
        let usart = unsafe { &*device::USART2::ptr() };

        // The UART has clock and is out of reset, but isn't actually on until we:
        usart.cr1.write(|w| w.ue().enabled());
        // Work out our baud rate divisor.
        const BAUDRATE: u32 = 115_200;

        #[cfg(feature = "stm32f3")]
        {
            const CLOCK_HZ: u32 = 8_000_000;
            usart
                .brr
                .write(|w| w.brr().bits((CLOCK_HZ / BAUDRATE) as u16));
        }

        #[cfg(feature = "stm32f4")]
        {
            const CLOCK_HZ: u32 = 16_000_000;
            const CYCLES_PER_BIT: u32 = (CLOCK_HZ + (BAUDRATE / 2)) / BAUDRATE;
            usart.brr.write(|w| {
                w.div_mantissa()
                    .bits((CYCLES_PER_BIT >> 4) as u16)
                    .div_fraction()
                    .bits(CYCLES_PER_BIT as u8 & 0xF)
            });
        }

        // Enable the transmitter.
        usart.cr1.modify(|_, w| w.te().enabled());

        turn_on_gpioa();

        // TODO: the fact that we interact with GPIOA directly here is an expedient
        // hack, but control of the GPIOs should probably be centralized somewhere.
        let gpioa = unsafe { &*device::GPIOA::ptr() };

        // Mux the USART onto the output pins. We're using PA2/3, where USART2 is
        // selected by Alternate Function 7.
        gpioa
            .moder
            .modify(|_, w| w.moder2().alternate().moder3().alternate());
        gpioa.afrl.modify(|_, w| w.afrl2().af7().afrl3().af7());

        let txref = Transmit {
            buffer: [0u8; STR_MAX_SIZE],
            len: 0,
            pos: 0,
        };

        Ok(Self { usart, txref })
    }
}

impl InOrderUsartApiImpl for ServerImpl {
    fn write(
        &mut self,
        _msg: &userlib::RecvMessage,
        data: LenLimit<Leased<R, [u8]>, STR_MAX_SIZE>,
    ) -> Result<(), idol_runtime::RequestError<UsartApiError>> {
        let len = data.len();

        // Deny incoming writes if we're already running one.
        if self.txref.len != 0 {
            return Err(UsartApiError::Busy.into());
        }

        if len > STR_MAX_SIZE {
            return Err(UsartApiError::BadTransferSize.into());
        }

        data
        .read_range(0..len, &mut self.txref.buffer)
        .map_err(|_| RequestError::Fail(ClientError::WentAway))?;

        self.txref.len = len;

        // OR the TX register empty signal into the USART interrupt.
        self.usart.cr1.modify(|_, w| w.txeie().enabled());

        // We'll do the rest as interrupts arrive.
        Ok(())
    }
}

impl NotificationHandler for ServerImpl {
    fn current_notification_mask(&self) -> u32 {
        notifications::USART_IRQ_MASK
    }

    fn handle_notification(&mut self, bits: u32) {
        if bits & 1 != 0 {
            // Handling an interrupt. To allow for spurious interrupts,
            // check the individual conditions we care about, and
            // unconditionally re-enable the IRQ at the end of the handler.

            #[cfg(feature = "stm32f3")]
            let txe = self.usart.isr.read().txe().bit();
            #[cfg(feature = "stm32f4")]
            let txe = self.usart.sr.read().txe().bit();
            if txe {
                // TX register empty. Do we need to send something?
                step_transmit(self.usart, &mut self.txref);
            }

            sys_irq_control(notifications::USART_IRQ_MASK, true);
        }
    }
}

include!(concat!(env!("OUT_DIR"), "/server_stub.rs"));
include!(concat!(env!("OUT_DIR"), "/notifications.rs"));
