// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use std::env;
use std::fs::File;
use std::io::Write;
use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    idol::Generator::new()
        .with_counters(
            idol::CounterSettings::default().with_server_counters(false),
        )
        .build_server_support(
            "../../idl/usart.idol",
            "server_stub.rs",
            idol::server::ServerStyle::InOrder,
        )?;

    build_util::build_notifications()?;

    let mut file = {
        let out = PathBuf::from(env::var_os("OUT_DIR").unwrap());
        File::create(out.join("peripheral_config.rs")).unwrap()
    };

    let peripherals = build_util::task_peripherals();

    if peripherals.len() != 1 {
        panic!("Support only 1 peripheral");
    }

    let (_, periph) = peripherals.first_key_value().unwrap();

    writeln!(
        file,
        "const UART_BASE_ADDR: u32 = 0x{:X}_u32;\n",
        periph.address
    )?;

    Ok(())
}
