// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Thermal loop
//!
//! This is a primordial thermal loop, which will ultimately reading temperature
//! sensors and control fan duty cycles to actively manage thermals.  Right now,
//! though it is merely reading every fan and temp sensor that it can find...
//!

#![no_std]
#![no_main]

mod bsp;
mod control;

use crate::{
    bsp::{Bsp, SeqError},
    control::ThermalControl,
};
use core::convert::TryFrom;
use drv_i2c_api::ResponseCode;
use drv_i2c_devices::max31790::I2cWatchdog;
use idol_runtime::{NotificationHandler, RequestError};
use ringbuf::*;
use task_sensor_api::{Sensor as SensorApi, SensorError, SensorId};
use task_thermal_api::{ThermalAutoState, ThermalError, ThermalMode};
use userlib::units::PWMDuty;
use userlib::*;

// We define our own Fan type, as we may have more fans than any single
// controller supports.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct Fan(u8);

impl From<usize> for Fan {
    fn from(index: usize) -> Self {
        Fan(index as u8)
    }
}

task_slot!(I2C, i2c_driver);
task_slot!(SENSOR, sensor);

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum Trace {
    None,
    Start,
    ThermalMode(ThermalMode),
    AutoState(ThermalAutoState),
    FanReadFailed(usize, ResponseCode),
    MiscReadFailed(usize, ResponseCode),
    SensorReadFailed(usize, ResponseCode),
    PostFailed(SensorId, SensorError),
    ControlPwm(u8),
    PowerModeChanged(u32),
    PowerDownFailed(SeqError),
}
ringbuf!(Trace, 32, Trace::None);

////////////////////////////////////////////////////////////////////////////////

struct ServerImpl<'a> {
    mode: ThermalMode,
    control: ThermalControl<'a>,
    deadline: u64,
}

const TIMER_MASK: u32 = 1 << 0;
const TIMER_INTERVAL: u64 = 1000;

impl<'a> ServerImpl<'a> {
    /// Configures the control loop to run in manual mode, loading the given
    /// PWM value immediately to all fans.
    ///
    /// Returns an error if the PWM code is invalid (> 100) or communication
    /// with any fan fails.
    fn set_mode_manual(
        &mut self,
        initial_pwm: PWMDuty,
    ) -> Result<(), ThermalError> {
        self.set_mode(ThermalMode::Manual);
        self.control.set_pwm(initial_pwm)
    }

    /// Configures the control loop to run in automatic mode.
    ///
    /// The fans will not change speed until the next controller update tick.
    ///
    /// Returns an error if the given PWM value is invalid.
    fn set_mode_auto(&mut self) -> Result<(), ThermalError> {
        if self.mode != ThermalMode::Auto {
            self.set_mode(ThermalMode::Auto);
            self.control.reset();
            Ok(())
        } else {
            Err(ThermalError::AlreadyInAutoMode)
        }
    }

    fn set_mode(&mut self, m: ThermalMode) {
        self.mode = m;
        ringbuf_entry!(Trace::ThermalMode(m));
    }

    fn set_watchdog(&self, wd: I2cWatchdog) -> Result<(), ThermalError> {
        self.control
            .set_watchdog(wd)
            .map_err(|_| ThermalError::DeviceError)
    }
}

impl<'a> idl::InOrderThermalImpl for ServerImpl<'a> {
    fn get_mode(
        &mut self,
        _: &RecvMessage,
    ) -> Result<ThermalMode, RequestError<ThermalError>> {
        Ok(self.mode)
    }

    fn get_auto_state(
        &mut self,
        _: &RecvMessage,
    ) -> Result<ThermalAutoState, RequestError<ThermalError>> {
        if self.mode != ThermalMode::Auto {
            return Err(ThermalError::NotInAutoMode.into());
        }
        Ok(self.control.get_state())
    }

    fn set_fan_pwm(
        &mut self,
        _: &RecvMessage,
        index: u8,
        pwm: u8,
    ) -> Result<(), RequestError<ThermalError>> {
        if self.mode != ThermalMode::Manual {
            return Err(ThermalError::NotInManualMode.into());
        }
        let pwm =
            PWMDuty::try_from(pwm).map_err(|_| ThermalError::InvalidPWM)?;

        if let Some(fan) = self.control.fan(index) {
            self.control
                .set_fan_pwm(fan, pwm)
                .map_err(|_| ThermalError::DeviceError.into())
        } else {
            Err(ThermalError::InvalidFan.into())
        }
    }

    fn set_mode_manual(
        &mut self,
        _: &RecvMessage,
        initial_pwm: u8,
    ) -> Result<(), RequestError<ThermalError>> {
        // Delegate to inner function after doing type conversions
        let initial_pwm = PWMDuty::try_from(initial_pwm)
            .map_err(|_| ThermalError::InvalidPWM)?;
        ServerImpl::set_mode_manual(self, initial_pwm).map_err(Into::into)
    }

    fn set_mode_auto(
        &mut self,
        _: &RecvMessage,
    ) -> Result<(), RequestError<ThermalError>> {
        ServerImpl::set_mode_auto(self).map_err(Into::into)
    }

    fn disable_watchdog(
        &mut self,
        _: &RecvMessage,
    ) -> Result<(), RequestError<ThermalError>> {
        ServerImpl::set_watchdog(self, I2cWatchdog::Disabled)
            .map_err(Into::into)
    }

    fn enable_watchdog(
        &mut self,
        _: &RecvMessage,
        timeout_s: u8,
    ) -> Result<(), RequestError<ThermalError>> {
        let wd = match timeout_s {
            5 => I2cWatchdog::FiveSeconds,
            10 => I2cWatchdog::TenSeconds,
            30 => I2cWatchdog::ThirtySeconds,
            _ => return Err(ThermalError::InvalidWatchdogTime.into()),
        };
        ServerImpl::set_watchdog(self, wd).map_err(Into::into)
    }

    fn set_pid(
        &mut self,
        _: &RecvMessage,
        p: f32,
        i: f32,
        d: f32,
    ) -> Result<(), RequestError<ThermalError>> {
        if self.mode != ThermalMode::Auto {
            return Err(ThermalError::NotInAutoMode.into());
        }
        self.control.set_pid(p, i, d)?;
        Ok(())
    }

    fn set_margin(
        &mut self,
        _: &RecvMessage,
        margin: f32,
    ) -> Result<(), RequestError<ThermalError>> {
        if self.mode != ThermalMode::Auto {
            return Err(ThermalError::NotInAutoMode.into());
        }
        self.control.set_margin(margin)?;
        Ok(())
    }

    fn get_margin(
        &mut self,
        _: &RecvMessage,
    ) -> Result<f32, RequestError<ThermalError>> {
        if self.mode != ThermalMode::Auto {
            return Err(ThermalError::NotInAutoMode.into());
        }
        Ok(self.control.get_margin())
    }
}

impl<'a> NotificationHandler for ServerImpl<'a> {
    fn current_notification_mask(&self) -> u32 {
        TIMER_MASK
    }

    fn handle_notification(&mut self, _bits: u32) {
        let now = sys_get_timer().now;
        if now >= self.deadline {
            match self.mode {
                ThermalMode::Auto => {
                    self.control.run_control(now);
                }
                ThermalMode::Manual => {
                    // Read sensors and post them to the `sensors` task
                    self.control.read_sensors(now);
                }
                ThermalMode::Off => {
                    panic!("Mode must not be 'Off' when server is running")
                }
            }
            self.deadline = now + TIMER_INTERVAL;
        }
        sys_set_timer(Some(self.deadline), TIMER_MASK);
    }
}

#[export_name = "main"]
fn main() -> ! {
    let i2c_task = I2C.get_task_id();
    let sensor_api = SensorApi::from(SENSOR.get_task_id());

    ringbuf_entry!(Trace::Start);

    let bsp = Bsp::new(i2c_task);
    let control = ThermalControl::new(&bsp, sensor_api);

    // This will put our timer in the past, and should immediately kick us.
    let deadline = sys_get_timer().now;
    sys_set_timer(Some(deadline), TIMER_MASK);

    let mut server = ServerImpl {
        mode: ThermalMode::Off,
        control,
        deadline,
    };
    if bsp::USE_CONTROLLER {
        server.set_mode_auto().unwrap();
    } else {
        server.set_mode_manual(PWMDuty(0)).unwrap();
    }

    let mut buffer = [0; idl::INCOMING_SIZE];
    loop {
        idol_runtime::dispatch_n(&mut buffer, &mut server);
    }
}

mod idl {
    use super::{ThermalAutoState, ThermalError, ThermalMode};

    include!(concat!(env!("OUT_DIR"), "/server_stub.rs"));
}
