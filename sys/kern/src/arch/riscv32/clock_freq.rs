// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use crate::time::Timestamp;

/// To allow our clock frequency to be easily determined from a debugger, we
/// store it in memory.
#[no_mangle]
pub static mut CLOCK_FREQ_KHZ: u32 = 0;

// Because debuggers need to know the clock frequency to set the SWO clock
// scaler that enables ITM, and because ITM is particularly useful when
// debugging boot failures, this should be set as early in boot as it can
// be.
pub fn set_clock_freq(tick_divisor: u32) {
    // TODO switch me to an atomic. Note that this may break Humility.
    // SAFETY:
    // In a single-threaded, single-process context (which the kernel is in),
    // access to global mutables are safe as data races are impossible.
    unsafe {
        CLOCK_FREQ_KHZ = tick_divisor;
    }
}

#[used]
pub static mut TICKS: u64 = 0;

/// Reads the tick counter.
pub fn now() -> Timestamp {
    Timestamp::from(unsafe { TICKS })
}
