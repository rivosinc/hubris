#![no_std]


#[allow(unused_imports)]
use userlib::*;
// use zerocopy::AsBytes;

use core::ptr::{slice_from_raw_parts_mut};

const WDOG_REGWEN: usize = 4;
const WDOG_CTRL: usize = 5;
const WDOG_BARK_THOLD: usize = 6;
const WDOG_BITE_THOLD: usize = 7;
const WDOG_COUNT: usize = 8;
const INTR_STATE: usize = 9;

// The OpenTitan Always-On Timer is a counter that increments. If the counter
// reaches a configurable threshold, it will emit an interrupt signal, called the
// "bark". If the counter reaches a second configurable threshold, it will reset
// the system, called the "bite".
// Enabling the AON Timer before setting a bite threshold causes the system to
// immediately reset.
pub struct AonTimer {
    base_addr: *mut [u32],
    clock_freq: u32,
    pub bark_cb: Option<fn()>,
}

impl AonTimer {

    // Initialize a new watchdog driver device with the specified base address,
    // clock frequency (in Hz), and the bark/bite thresholds (both in ms).
    // If the bark threshold > bite threshold, the bite threshold is set to
    // the bark threshold.
    pub fn new(base_addr: u32, clock_freq: u32, bark_thold_ms: u64, bite_thold_ms: u64, bark_cb: Option<fn()>) -> Self {
        let inst = AonTimer {
            base_addr: slice_from_raw_parts_mut(base_addr as *mut u32, 12),
            clock_freq,
            bark_cb,
        };
        unsafe {        
        //1: init regs
            (*inst.base_addr)[WDOG_COUNT] = 0;
            (*inst.base_addr)[INTR_STATE] = 0b11;
            (*inst.base_addr)[WDOG_BARK_THOLD] = inst.ms_to_reg(bark_thold_ms);
            if bark_thold_ms > bite_thold_ms {
                (*inst.base_addr)[WDOG_BITE_THOLD] = inst.ms_to_reg(bark_thold_ms);
            } else {
                (*inst.base_addr)[WDOG_BITE_THOLD] = inst.ms_to_reg(bite_thold_ms);
            }
        }
        inst
    }

    //Disable the watchdog timer. If the configuration is locked, returns false.
    pub fn disable(&self) -> bool {
        if !self.check_lock() {
            return false;
        }
        unsafe {
            (*self.base_addr)[WDOG_CTRL] = 0;
        }
        true
    }

    //Enables the watchdog timer. If the configuration is locked, returns false.
    pub fn enable(&self) -> bool {
        if !self.check_lock() {
            return false;
        }
        unsafe {
            (*self.base_addr)[WDOG_CTRL] |= 1;
        }
        true
    }

    //Enables the watchdog timer, then locks the configuration. If the
    // configuration is already locked, returns false.
    pub fn enable_and_lock(&self) -> bool {
        if !self.check_lock() {
            return false;
        }
        unsafe {
            (*self.base_addr)[WDOG_CTRL] |= 1;
            (*self.base_addr)[WDOG_REGWEN] = 0;
        }
        true
    }

    //Sets the bark threshold in ms. If the configuration is locked, returns false.
    pub fn set_bark_thold(&self, bark_thold_ms: u64) -> bool {
        if !self.check_lock() {
            return false;
        }
        unsafe {
            (*self.base_addr)[WDOG_BARK_THOLD] = self.ms_to_reg(bark_thold_ms);
        }
        true
    }

    //Sets the bite threshold in ms. If the configuration is locked, returns false.
    pub fn set_bite_thold(&self, bite_thold_ms: u64) -> bool {
        if !self.check_lock() {
            return false;
        }
        unsafe {
            (*self.base_addr)[WDOG_BITE_THOLD] = self.ms_to_reg(bite_thold_ms);
        }
        true
    }

    //Pets the watchdog, i.e. resets the count down to 0.
    pub fn feed_sacrifice(&self) {
        unsafe {
            (*self.base_addr)[WDOG_COUNT] = 0;
        }
    }

    pub fn clear_wdt_irq(&self) {
        unsafe {
            (*self.base_addr)[INTR_STATE] |= 0x2;
        }
    }

    //Checks whether the configuration is locked.
    #[inline(always)]
    fn check_lock(&self) -> bool {
        unsafe {
            (*self.base_addr)[WDOG_REGWEN] == 1
        }
    }

    // Converts  milliseconds to an appropriate count in the register.
    // If the result is greater than max int, the value is saturated to max int.
    fn ms_to_reg(&self, ms: u64) -> u32 {
        let mut time: u64 = ms * ((self.clock_freq / 1000) as u64);
        if time > u32::MAX.into() {
            time = u32::MAX.into();
        }
        time.try_into().unwrap()
    }
}

