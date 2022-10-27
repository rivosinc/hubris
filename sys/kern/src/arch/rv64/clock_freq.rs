// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

/// To allow our clock frequency to be easily determined from a debugger, we
/// store it in memory.
#[no_mangle]
pub static mut CLOCK_FREQ_KHZ: u32 = 0;

// Because debuggers need to know the clock frequency to set the SWO clock
// scaler that enables ITM, and because ITM is particularly useful when
// debugging boot failures, this should be set as early in boot as it can
// be.
pub unsafe fn set_clock_freq(tick_divisor: u32) {
    // TODO switch me to an atomic. Note that this may break Humility.
    unsafe {
        CLOCK_FREQ_KHZ = tick_divisor;
    }
}
