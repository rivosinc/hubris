// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use crate::task;
use riscv::register;

pub fn apply_memory_protection(task: &task::Task) {
    use riscv::register::{Mode, Permission, PmpCfg};

    let null_cfg: PmpCfg = PmpCfg::new(Mode::OFF, Permission::NONE, false);

    for (i, region) in task.region_table().iter().enumerate() {
        if (region.base == 0x0) && (region.size == 0x20) {
            continue;
        }
        let pmpcfg = {
            let pmp_perm: Permission = match region.attributes.bits() & 0b111 {
                0b000 => Permission::NONE,
                0b001 => Permission::R,
                0b010 => panic!(),
                0b011 => Permission::RW,
                0b100 => Permission::X,
                0b101 => Permission::RX,
                0b110 => panic!(),
                0b111 => Permission::RWX,
                _ => unreachable!(),
            };

            PmpCfg::new(Mode::TOR, pmp_perm, false)
        };

        unsafe {
            // Configure the base address entry
            register::set_cfg_entry(i * 2, null_cfg);
            register::write_tor_indexed(i * 2, region.base as u64);

            // Configure the end address entry
            register::set_cfg_entry(i * 2 + 1, pmpcfg);
            register::write_tor_indexed(
                i * 2 + 1,
                (region.base + region.size) as u64,
            );
        }
    }
}
