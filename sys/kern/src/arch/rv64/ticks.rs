// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use crate::time::Timestamp;

#[used]
static mut TICKS: u64 = 0;

pub fn incr_ticks(incr: u64) -> Timestamp {
    let ticks = unsafe { &mut TICKS };
    *ticks += incr;
    drop(ticks);
    now()
}

/// Reads the tick counter.
pub fn now() -> Timestamp {
    Timestamp::from(unsafe { TICKS })
}
