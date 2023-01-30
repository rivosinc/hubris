// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

#![no_std]

use drv_int_chip_api::IntChipApi;
use drv_timer_api::TimerApi;

/// Trait to chip functionnalities
pub trait ChipApi: TimerApi + IntChipApi {
    /// restart the CPU
    fn reset(&self) -> ! {
        unimplemented!();
    }
}
