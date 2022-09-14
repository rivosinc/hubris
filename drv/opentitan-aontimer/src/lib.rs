// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

#![no_std]

use core::ptr::slice_from_raw_parts_mut;

const WDOG_REGWEN_IDX: usize = 4;
const WDOG_CTRL_IDX: usize = 5;
const WDOG_BARK_THOLD_IDX: usize = 6;
const WDOG_BITE_THOLD_IDX: usize = 7;
const WDOG_COUNT_IDX: usize = 8;
const INTR_STATE_IDX: usize = 9;

// The OpenTitan Always-On Timer is a counter that increments. If the counter
// reaches a configurable threshold, it will emit an interrupt signal, called the
// "bark". If the counter reaches a second configurable threshold, it will reset
// the system, called the "bite".
// Enabling the AON Timer before setting a bite threshold causes the system to
// immediately reset.
pub struct AonTimer {
    base_addr: *mut [u32],
    clock_freq_hz: u32,
    pub bark_cb: Option<fn()>,
}

impl AonTimer {
    // Initialize a new watchdog driver device with the specified base address,
    // clock frequency (in Hz), and the bark/bite thresholds (both in ms).
    // If the bark threshold > bite threshold, the bite threshold is set to
    // the bark threshold.
    pub fn new(
        base_addr: u32,
        clock_freq_hz: u32,
        bark_thold_ms: u64,
        bite_thold_ms: u64,
        bark_cb: Option<fn()>,
    ) -> Result<Self, ()> {
        let inst = AonTimer {
            base_addr: slice_from_raw_parts_mut(base_addr as *mut u32, 12),
            clock_freq_hz,
            bark_cb,
        };
        unsafe {
            (*inst.base_addr)[WDOG_COUNT_IDX] = 0;
            (*inst.base_addr)[INTR_STATE_IDX] = 0b11;
            (*inst.base_addr)[WDOG_BARK_THOLD_IDX] = inst.ms_to_reg_count(bark_thold_ms)?;
            if bark_thold_ms > bite_thold_ms {
                return Err(());
            }
            (*inst.base_addr)[WDOG_BITE_THOLD_IDX] = inst.ms_to_reg_count(bite_thold_ms)?;
        }
        Ok(inst)
    }

    //Disable the watchdog timer. If the configuration is locked, returns false.
    pub fn disable(&self) -> Result<(), ()> {
        if !self.check_lock() {
            return Err(());
        }
        unsafe {
            (*self.base_addr)[WDOG_CTRL_IDX] = 0;
        }
        Ok(())
    }

    //Enables the watchdog timer. If the configuration is locked, returns false.
    pub fn enable(&self) -> Result<(), ()> {
        if !self.check_lock() {
            return Err(());
        }
        unsafe {
            (*self.base_addr)[WDOG_CTRL_IDX] |= 1;
        }
        Ok(())
    }

    //Enables the watchdog timer, then locks the configuration. If the
    // configuration is already locked, returns false.
    pub fn enable_and_lock(&self) -> Result<(), ()>{
        if !self.check_lock() {
            return Err(());
        }
        unsafe {
            (*self.base_addr)[WDOG_CTRL_IDX] |= 1;
            (*self.base_addr)[WDOG_REGWEN_IDX] = 0;
        }
        Ok(())
    }

    //Sets the bark threshold in ms. If the configuration is locked, returns false.
    pub fn set_bark_thold(&self, bark_thold_ms: u64) -> Result<(), ()>{
        if !self.check_lock() {
            return Err(());
        }
        unsafe {
            (*self.base_addr)[WDOG_BARK_THOLD_IDX] =
                self.ms_to_reg_count(bark_thold_ms)?;
        }
        Ok(())
    }

    //Sets the bite threshold in ms. If the configuration is locked, returns false.
    pub fn set_bite_thold(&self, bite_thold_ms: u64) -> Result<(), ()> {
        if !self.check_lock() {
            return Err(());
        }
        unsafe {
            (*self.base_addr)[WDOG_BITE_THOLD_IDX] =
                self.ms_to_reg_count(bite_thold_ms)?;
        }
        Ok(())
    }

    //Pets the watchdog, i.e. resets the count down to 0.
    pub fn feed_sacrifice(&self) {
        unsafe {
            (*self.base_addr)[WDOG_COUNT_IDX] = 0;
        }
    }

    pub fn clear_wdt_irq(&self) {
        unsafe {
            (*self.base_addr)[INTR_STATE_IDX] |= 0x2;
        }
    }

    //Checks whether the configuration is locked.
    #[inline(always)]
    fn check_lock(&self) -> bool {
        unsafe { (*self.base_addr)[WDOG_REGWEN_IDX] == 1 }
    }

    // Converts  milliseconds to an appropriate count in the register.
    // If the result is greater than max int, the value is saturated to max int.
    fn ms_to_reg_count(&self, ms: u64) -> Result<u32, ()> {
        let mut time: u64 = ms * ((self.clock_freq_hz / 1000) as u64);
        time.try_into().map_err(|_e| ())
    }
}
