// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
//
use std::env;
use std::fs::File;
use std::io::Write;
use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut file = {
        let out = PathBuf::from(env::var_os("OUT_DIR").unwrap());
        File::create(out.join("config.rs")).unwrap()
    };

    // This function generates notification bitmask constants for IRQs. It can
    // be removed if this task doesn't handle any interrupts.
    writeln!(file, "{}", build_util::task_irq_consts())?;
    // This function generates constants for the base address and size of
    // peripherals. It can be removed if the task doesn't use any.
    writeln!(file, "{}", build_util::task_peripherals_str())?;

    Ok(())
}
