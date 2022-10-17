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
        File::create(out.join("rtc_config.rs")).unwrap()
    };

    writeln!(file, "{}", build_util::task_irq_consts())?;
    writeln!(file, "{}", build_util::task_peripherals_str())?;

    Ok(())
}
