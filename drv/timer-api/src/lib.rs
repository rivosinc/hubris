// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

#![no_std]

/// Trait to work with the systick timer
pub trait TimerApi {
    /// Return the current time.
    fn get_time(&self) -> u64;

    /// Set the next timer (absolute time).
    fn set_timer(&mut self, timer: u64);

    /// Increase the timer (relative time).
    fn increase_timer(&mut self, delta: u64);
}
