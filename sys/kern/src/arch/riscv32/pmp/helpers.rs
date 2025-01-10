// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use crate::descs::RegionAttributes;
#[cfg(any(feature = "riscv-epmp-mml", feature = "riscv-epmp-mmwp"))]
use crate::umem::USlice;
use cfg_if::cfg_if;
use riscv::register::{
    pmpaddr0, pmpaddr1, pmpaddr10, pmpaddr11, pmpaddr12, pmpaddr13, pmpaddr14,
    pmpaddr15, pmpaddr2, pmpaddr3, pmpaddr4, pmpaddr5, pmpaddr6, pmpaddr7,
    pmpaddr8, pmpaddr9, pmpcfg0, pmpcfg1, pmpcfg2, pmpcfg3, Pmpcsr,
};

#[derive(Copy, Clone, Debug)]
#[repr(C)]
pub struct PmpCfg {
    pub cfg_bits: u8,
    pub addr: u32,
    pub prev_addr: Option<u32>,
}

pub const fn compute_pmp_cfg(
    addr: u32,
    size: u32,
    attributes: RegionAttributes,
    is_kern_region: bool,
    locked: bool,
) -> PmpCfg {
    if size == 0 {
        // nothing to compute, just need an empty PMP
        return PmpCfg {
            cfg_bits: 0,
            addr: 0,
            prev_addr: None,
        };
    }
    // compute PMP range
    let (range_cfg, addr, prev_addr) = if size == 4 && addr & 0x03 == 0 {
        // NA4
        (2, addr >> 2, None)
    } else {
        let addr_align = addr.trailing_zeros();
        let size_power = size.trailing_zeros();
        // verify if it can be encoded in NAPOT
        if size == 1 << size_power && addr_align >= size_power {
            // NAPOT
            (3, (addr >> 2) | ((size - 1) >> 3), None)
        } else {
            // TOR
            (1, (addr + size) >> 2, Some(addr >> 2))
        }
    };

    cfg_if! {
        if #[cfg(feature = "riscv-epmp-mml")] {
            const R: RegionAttributes = RegionAttributes::READ;
            const W: RegionAttributes = RegionAttributes::WRITE;
            const X: RegionAttributes = RegionAttributes::EXECUTE;
            const RW: RegionAttributes = R.union(W);
            const RX: RegionAttributes = R.union(X);
            const RWX: RegionAttributes = RW.union(X);
            let req_perm = attributes.intersection(RWX);
            let perm = match (is_kern_region, locked, req_perm) {
                (false, false, X)   => 0b100,
                (false, false, R)   => 0b001,
                (false, false, RX)  => 0b101,
                (false, false, RW)  => 0b011,
                (false, false, RWX) => 0b111,
                (false, true,  X)   => 0b010,
                (false, true,  R)   => 0b111,

                (true,  true,  X)   => 0b100,
                (true,  true,  R)   => 0b001,
                (true,  true,  RX)  => 0b101,
                (true,  true,  RW)  => 0b011,
                (true,  false, RW)  => 0b010,
                // Unsupported mode
                (   _,  _,     _)   => 0b000,
            };
        } else {
            // ignore unused argument
            let _ = is_kern_region;

            // convert permissions
            const READ_PERM: u8 = 1 << 0;
            const WRITE_PERM: u8 = 1 << 1;
            const EXECUTE_PERM: u8 = 1 << 2;
            let mut perm = 0;
            if attributes.contains(RegionAttributes::READ) {
                perm |= READ_PERM;
            }
            if attributes.contains(RegionAttributes::WRITE) {
                perm |= WRITE_PERM;
            }
            if attributes.contains(RegionAttributes::EXECUTE) {
                perm |= EXECUTE_PERM;
            }
        }
    }
    let cfg_bits = (locked as u8) << 7 | range_cfg << 3 | perm;

    PmpCfg {
        cfg_bits,
        addr,
        prev_addr,
    }
}

#[cfg(any(feature = "riscv-epmp-mml", feature = "riscv-epmp-mmwp"))]
pub(super) type PmpBackup = [(usize, usize); 2];

/// Turn the cfg at the provided index to off.
/// # Safety: ensure no accesses will occur to this region anymore
pub(super) unsafe fn clear_pmp_cfg(index: usize) {
    let cfg_clear = pmp_cfg_clear_func(index);
    let byte_idx = index % 4;
    unsafe { cfg_clear(byte_idx) };
}

pub(super) fn write_pmp_cfg(mut index: usize, cfg: &PmpCfg) -> usize {
    if let Some(addr) = cfg.prev_addr {
        // write TOR first slot if necessary
        let cfg_clear = pmp_cfg_clear_func(index);
        let byte_idx = index % 4;
        unsafe { cfg_clear(byte_idx) };

        let addr_write = pmp_addr_write_func(index);
        addr_write(addr as usize);

        index += 1;
    }

    let cfg_read = pmp_cfg_read_func(index);
    let cfg_write = pmp_cfg_write_func(index);
    let byte_idx = index % 4;
    let shift = byte_idx * 8;
    let mut cfg_value = cfg_read().bits;

    // start by clearing configuration
    cfg_value &= !(0xFF << shift);
    cfg_write(cfg_value);

    // write addresses
    let addr_write = pmp_addr_write_func(index);
    addr_write(cfg.addr as usize);

    // write final configuration
    cfg_value |= (cfg.cfg_bits as usize) << shift;
    cfg_write(cfg_value);

    index + 1
}

/// Open a PMP slot for a region but back up the slot(s) in the process
#[cfg(any(feature = "riscv-epmp-mml", feature = "riscv-epmp-mmwp"))]
pub(super) fn save_and_replace_pmp_cfg(
    config: &PmpCfg,
    index: usize,
) -> PmpBackup {
    let mut backup = PmpBackup::default();
    for (i, slot) in backup.iter_mut().enumerate() {
        let slot_idx = index + i;
        let cfg_read = pmp_cfg_read_func(slot_idx);
        let cfg_clear = pmp_cfg_clear_func(slot_idx);
        let addr_read = pmp_addr_read_func(slot_idx);

        let byte_idx = slot_idx % 4;
        let shift = byte_idx * 8;

        // save current slot
        let reg = cfg_read().bits;
        let saved_cfg = reg >> shift & 0xFF;
        let saved_addr = addr_read();
        *slot = (saved_cfg, saved_addr);

        // clear config
        unsafe { cfg_clear(byte_idx) };
    }

    if config.cfg_bits == 0 {
        // no need to open any PMP
        return backup;
    }

    write_pmp_cfg(index, config);

    backup
}

/// Restore a previously backed up slot(s)
#[cfg(any(feature = "riscv-epmp-mml", feature = "riscv-epmp-mmwp"))]
pub(super) fn restore_pmp_backup(index: usize, backup: &PmpBackup) {
    for (i, slot) in backup.iter().enumerate() {
        let slot_idx = index + i;
        let cfg_read = pmp_cfg_read_func(slot_idx);
        let cfg_write = pmp_cfg_write_func(slot_idx);
        let addr_write = pmp_addr_write_func(slot_idx);

        let byte_idx = slot_idx % 4;
        let shift = byte_idx * 8;

        let (saved_cfg, saved_addr) = *slot;

        let reg = cfg_read().bits;
        let reg = reg & !(0xFF << shift) | saved_cfg << shift;
        addr_write(saved_addr);
        cfg_write(reg);
    }
}

/// extract base address and size of the slice and contruct a memory region
/// around it
#[cfg(any(feature = "riscv-epmp-mml", feature = "riscv-epmp-mmwp"))]
pub(super) fn uslice_to_pmp_cfg<T>(slice: &USlice<T>) -> PmpCfg {
    // align start address on 32 bytes
    let aligned_base_addr = slice.base_addr() as u32 & 0xFFFF_FFFC;
    // align end address on 32 bytes
    let aligned_end_addr = slice.end_addr().next_multiple_of(4) as u32;
    // compute aligned size
    let size = aligned_end_addr - aligned_base_addr;
    let attributes = RegionAttributes::READ | RegionAttributes::WRITE;

    compute_pmp_cfg(aligned_base_addr, size, attributes, true, false)
}

fn pmp_cfg_clear_func(index: usize) -> unsafe fn(usize) {
    match index / 4 {
        0 => pmpcfg0::clear_pmp,
        1 => pmpcfg1::clear_pmp,
        2 => pmpcfg2::clear_pmp,
        3 => pmpcfg3::clear_pmp,
        _ => panic!("Invalid index"),
    }
}

fn pmp_cfg_read_func(index: usize) -> fn() -> Pmpcsr {
    match index / 4 {
        0 => pmpcfg0::read,
        1 => pmpcfg1::read,
        2 => pmpcfg2::read,
        3 => pmpcfg3::read,
        _ => panic!("Invalid index"),
    }
}

fn pmp_cfg_write_func(index: usize) -> fn(usize) {
    match index / 4 {
        0 => pmpcfg0::write,
        1 => pmpcfg1::write,
        2 => pmpcfg2::write,
        3 => pmpcfg3::write,
        _ => panic!("Invalid index"),
    }
}

fn pmp_addr_write_func(index: usize) -> fn(usize) {
    match index {
        0 => pmpaddr0::write,
        1 => pmpaddr1::write,
        2 => pmpaddr2::write,
        3 => pmpaddr3::write,
        4 => pmpaddr4::write,
        5 => pmpaddr5::write,
        6 => pmpaddr6::write,
        7 => pmpaddr7::write,
        8 => pmpaddr8::write,
        9 => pmpaddr9::write,
        10 => pmpaddr10::write,
        11 => pmpaddr11::write,
        12 => pmpaddr12::write,
        13 => pmpaddr13::write,
        14 => pmpaddr14::write,
        15 => pmpaddr15::write,
        _ => panic!("Invalid index"),
    }
}

#[cfg(any(feature = "riscv-epmp-mml", feature = "riscv-epmp-mmwp"))]
fn pmp_addr_read_func(index: usize) -> fn() -> usize {
    match index {
        0 => pmpaddr0::read,
        1 => pmpaddr1::read,
        2 => pmpaddr2::read,
        3 => pmpaddr3::read,
        4 => pmpaddr4::read,
        5 => pmpaddr5::read,
        6 => pmpaddr6::read,
        7 => pmpaddr7::read,
        8 => pmpaddr8::read,
        9 => pmpaddr9::read,
        10 => pmpaddr10::read,
        11 => pmpaddr11::read,
        12 => pmpaddr12::read,
        13 => pmpaddr13::read,
        14 => pmpaddr14::read,
        15 => pmpaddr15::read,
        _ => panic!("Invalid index"),
    }
}
