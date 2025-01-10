// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

#[cfg(feature = "riscv-epmp-mmwp")]
use crate::arch::riscv32::chip;
use crate::descs::{RegionAttributes, RegionDesc};
use crate::task;
use crate::umem::USlice;
use cfg_if::cfg_if;
#[cfg(any(
    feature = "riscv-epmp-mml",
    feature = "riscv-epmp-mmwp",
    feature = "riscv-epmp-no-rlb"
))]
use riscv::register::mseccfg;

mod helpers;

const MAX_PMP_REGIONS: usize = 16;

/// First pmp region that is usable for protecting tasks
/// It is written only once before any access
static mut TASK_PMP_START_IDX: usize = 0;

pub type RegionDescExt = helpers::PmpCfg;

pub const fn compute_region_extension_data(
    base: u32,
    size: u32,
    attributes: RegionAttributes,
    is_kern_region: bool,
) -> RegionDescExt {
    // This function is called by the generated code, this code is used to declare static kernel
    // and task regions. In this case we know that kernel region must be locked and task regions
    // must not.
    let locked = is_kern_region;
    helpers::compute_pmp_cfg(base, size, attributes, is_kern_region, locked)
}

/// This sets up PMP slots for the kernel if required and clears out the other
/// regions
pub fn init_memory_protection(
    kernel_regions: &[RegionDesc],
    _task_table: &[task::Task],
) {
    // keep track of what pmps we have written to
    let mut pmp_idx: usize = 0;

    cfg_if! {
        if #[cfg(feature = "riscv-epmp-mmwp")] {
            for region in kernel_regions {
                pmp_idx = helpers::write_pmp_region(
                    region,
                    pmp_idx,
                    ExecTarget::Kernel,
                    true,
                );
            }
        } else {
            // ignore unused argument
            let _ = kernel_regions;
        }
    }

    // Save our last used pmp entry so the task pmps can pick up where we left
    // off.
    // #Safety: This is the only place it is set, and this is done before any
    // read
    unsafe {
        TASK_PMP_START_IDX = pmp_idx;
    }

    // We can clear all other entries to attempt to start from a clean slate.
    // Since we already added kernel regions, be will not lock ourselves out
    // regardless of mml and mmwp
    while pmp_idx < MAX_PMP_REGIONS {
        // # Safety: these regions are unused by the next task
        // and index is valid
        unsafe {
            helpers::clear_pmp_cfg(pmp_idx);
        }
        pmp_idx += 1;
    }

    // Now we can fully configure mseccfg for runtime operations
    //
    // # Safety: these config should be safe as kernel setup is done previously:

    // Enable MML to have different permissions between M-mode and U-mode
    #[cfg(feature = "riscv-epmp-mml")]
    unsafe {
        mseccfg::set_mml()
    };

    // Enable MMWP to prevent the m-mode from running anywhere but the allowed
    // regions
    #[cfg(feature = "riscv-epmp-mmwp")]
    unsafe {
        mseccfg::set_mmwp()
    };

    // Clear rlb as we do not need to, nor should we, overwrite locked regions
    // anymore
    #[cfg(feature = "riscv-epmp-no-rlb")]
    unsafe {
        mseccfg::clear_rlb()
    };
}

pub fn apply_memory_protection(task: &task::Task) {
    // #Safety: Written only once before this is executed
    let mut pmp_idx = unsafe { TASK_PMP_START_IDX };

    for region in task.region_table().iter() {
        // skip any filler regions, we will zero out any regions left over when
        // we are done
        if (region.base == 0) && (region.size == 0x20) {
            // TODO still needed?
            continue;
        }

        pmp_idx = helpers::write_pmp_cfg(pmp_idx, &region.arch_data);
    }

    // Now turn off any remaining entries
    while pmp_idx < MAX_PMP_REGIONS {
        // # Safety: these regions are unused by the next task
        unsafe {
            helpers::clear_pmp_cfg(pmp_idx);
        }
        pmp_idx += 1;
    }
}

/// Give the kernel temporary RW access to a USlice.
/// Calls to this function can be chained only once
pub fn with_uslice_mut<T, R>(
    slice: &mut USlice<T>,
    nested: bool,
    body: impl FnOnce(&mut USlice<T>) -> R,
) -> R {
    cfg_if! {
        if #[cfg(any(feature = "riscv-epmp-mml", feature = "riscv-epmp-mmwp"))] {
            // #Safety: Written only once before this is executed
            let mut pmp_idx = unsafe { TASK_PMP_START_IDX };
            // if this is nested this means the 2 first slot are already in use
            if nested {
                pmp_idx += 2;
            }

            // Temporarily replace a pmp region to give kernel access
            let config = helpers::uslice_to_pmp_cfg(slice);
            let backup =
                helpers::save_and_replace_pmp_cfg(&config, pmp_idx);

            // we now have access, so we can call the body
            let r = body(slice);

            // Restore replaced PMP regions
            helpers::restore_pmp_backup(pmp_idx, &backup);

            r
        } else {
            // ignore unused variable
            let _ = nested;

            body(slice)
        }
    }
}

/// Give the kernel temporary R access to a USlice.
/// Calls to this function can be chained only once
pub fn with_uslice<T, R>(
    slice: &USlice<T>,
    nested: bool,
    body: impl FnOnce(&USlice<T>) -> R,
) -> R {
    cfg_if! {
        if #[cfg(any(feature = "riscv-epmp-mml", feature = "riscv-epmp-mmwp"))] {
            // #Safety: Written only once before this is executed
            let mut pmp_idx = unsafe { TASK_PMP_START_IDX };
            // if this is nested this means the 2 first slot are already in use
            if nested {
                pmp_idx += 2;
            }

            // Temporarily replace a pmp region to give kernel access
            let config = helpers::uslice_to_pmp_cfg(slice);
            let backup = helpers::save_and_replace_pmp_cfg(&config, pmp_idx);

            // we now have access, so we can call the body
            let r = body(slice);

            // Restore replaced PMP regions
            helpers::restore_pmp_backup(pmp_idx, &backup);

            r
        } else {
            // ignore unused variable
            let _ = nested;

            body(slice)
        }
    }
}
