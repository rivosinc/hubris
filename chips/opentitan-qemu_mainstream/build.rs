// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use quote::quote;
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io::Write;
use std::ops::Rem;

use build_kconfig::KernelConfig;

// We consider that we want to be able to run for at least 100 years
const MAX_TICK_DIVIDOR: u32 = 1 << 22;

// we considere that harware is able to support 1KHz for the systick
const TICKS_PER_MS: u32 = 1000;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "kebab-case")]
struct GlobalConfig {
    pub timer: TimerConfig,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "kebab-case")]
struct TimerConfig {
    pub clock_frequency: u32,
    #[serde(default = "prescaler_default")]
    pub prescaler: u16,
}

const fn prescaler_default() -> u16 {
    1
}

#[derive(Default, Deserialize, Serialize)]
struct PlicConfig {
    pbits: u8,
    context_id: u32,
    max_irq: u32,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    build_util::build_kernel_peripherals()?;

    let file_path = build_util::out_dir().join("chip_config.rs");
    let mut file = File::create(&file_path).unwrap();

    // verify there is only one external interrupt controller, named "plic".
    // This way our chip will not verify the controller parameter
    let kconfig: KernelConfig =
        ron::de::from_str(&build_util::env_var("HUBRIS_KCONFIG")?)?;
    let ctrl_num = kconfig.external_interrupt_controllers.len();
    if ctrl_num != 1 {
        panic!("Opentitan chip supports exactly 1 external interrupt controller: plic");
    }
    // generate the list of used irqs and the maximum irq num
    if let Some(id) = kconfig.external_interrupt_controllers.get("plic") {
        let used_irq: Vec<_> = kconfig
            .irqs
            .iter()
            .filter_map(|(src, _)| {
                if src.controller == *id {
                    Some(src.irq)
                } else {
                    None
                }
            })
            .collect();
        let used_irq_len = used_irq.len();
        writeln!(
            file,
            "{}",
            quote::quote! {
                #[allow(unused)]
                const PLIC_USED_IRQ: [u32; #used_irq_len] = [#(#used_irq),*];
            }
        )?;
    } else {
        panic!("Opentitan chip supports only 'plic' as external interrupt controller");
    }

    // Get plic config and expose the priority size
    let (_, config) =
        build_util::kernel_peripheral_and_config::<PlicConfig>("plic")?;
    let pbits = config.pbits as u32;
    let context_id = config.context_id as usize;
    let max_irq = config.max_irq;
    writeln!(
        file,
        "{}",
        quote::quote! {
            const PLIC_PRIORITY_BITS: u32 = #pbits;
            const PLIC_CONTEXT_ID: usize = #context_id;
            const PLIC_MAX_IRQ: u32 = #max_irq;
        }
    )?;

    // Compute timer config values
    let cfg = build_util::config::<GlobalConfig>()?;
    let timer_freq = cfg.timer.clock_frequency;
    let timer_pre = cfg.timer.prescaler;
    if timer_pre == 0 {
        panic!("Timer prescaler must be > 0");
    }
    let divisor = TICKS_PER_MS * (timer_pre as u32);
    if timer_freq.rem(divisor) > 0 {
        panic!("Invalid timer prescaler, not an integer division");
    }
    let tick_divisor = timer_freq / divisor;
    if tick_divisor > MAX_TICK_DIVIDOR {
        panic!(
            "Computed tick divisor is to big ({}), increase prescaler",
            tick_divisor
        );
    }
    let timer_prescale = timer_pre - 1;

    writeln!(
        file,
        "{}",
        quote! {
            const TIMER_PRESCALE: u16 = #timer_prescale;
            const TIMER_STEP: u8 = 1;
            const TIMER_KERNEL_TICK_DIVISOR: u32 = #tick_divisor;
        }
    )?;

    // format output file
    drop(file);
    call_rustfmt::rustfmt(file_path)?;

    println!("cargo:rerun-if-changed=chip.toml");
    Ok(())
}
