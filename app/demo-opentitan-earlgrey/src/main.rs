// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

#![no_std]
#![no_main]

use chip_opentitan_earlgrey::OpenTitanChip;
use riscv_rt_wrapper::entry;

#[entry]
fn main() -> ! {
    static mut CHIP: Option<OpenTitanChip> = None;
    unsafe {
        CHIP = Some(OpenTitanChip::new());
        let chip_ref = CHIP.as_mut().unwrap();
        let tick_divisor = chip_ref.init();
        kern::arch::set_chip_reference(chip_ref);
        kern::startup::start_kernel(tick_divisor);
    };

}
