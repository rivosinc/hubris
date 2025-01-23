// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use quote::quote;
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io::Write;

use build_kconfig::KernelConfig;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "kebab-case")]
struct GlobalConfig {
    pub timer: TimerConfig,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "kebab-case")]
struct TimerConfig {
    pub clock_frequency: u32,
}

#[derive(Default, Deserialize, Serialize)]
struct PlicConfig {
    pbits: u8,
    context_id: u32,
    max_irq: u32,
}

const TICKS_PER_MS: u32 = 1000;

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
    if timer_freq == 0 {
        panic!("timer clock frequency must be > 0");
    }
    if timer_freq < TICKS_PER_MS {
        panic!("timer clock frequency must be superior of {}.", TICKS_PER_MS);
    }
    let tick_divisor = timer_freq / TICKS_PER_MS;

    writeln!(
        file,
        "{}",
        quote! {
            const TIMER_KERNEL_TICK_DIVISOR: u32 = #tick_divisor;
        }
    )?;

    // format output file
    drop(file);
    call_rustfmt::rustfmt(file_path)?;

    println!("cargo:rerun-if-changed=chip.toml");
    Ok(())
}
