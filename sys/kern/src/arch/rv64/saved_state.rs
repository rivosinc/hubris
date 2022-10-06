// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use crate::task;
use zerocopy::FromBytes;

/// RISC-V volatile registers that must be saved across context switches.
#[repr(C)]
#[derive(Clone, Debug, Default, FromBytes)]
pub struct SavedState {
    // NOTE: the following fields must be kept contiguous!
    ra: u64,
    sp: u64,
    gp: u64,
    tp: u64,
    t0: u64,
    t1: u64,
    t2: u64,
    s0: u64,
    s1: u64,
    a0: u64,
    a1: u64,
    a2: u64,
    a3: u64,
    a4: u64,
    a5: u64,
    a6: u64,
    a7: u64,
    s2: u64,
    s3: u64,
    s4: u64,
    s5: u64,
    s6: u64,
    s7: u64,
    s8: u64,
    s9: u64,
    s10: u64,
    s11: u64,
    t3: u64,
    t4: u64,
    t5: u64,
    t6: u64,
    // Additional save value for task program counter
    pc: u64,
    // NOTE: the above fields must be kept contiguous!
}

impl SavedState {
    pub fn sp(&self) -> u64 {
        self.sp
    }
    pub fn pc(&self) -> u64 {
        self.pc
    }
    pub fn set_sp(&mut self, val: u64) {
        self.sp = val;
    }
    pub fn set_pc(&mut self, val: u64) {
        self.pc = val;
    }
    pub fn arg7(&self) -> usize {
        self.a7 as usize
    }
}

/// Map the volatile registers to (architecture-independent) syscall argument
/// and return slots.
impl task::ArchState for SavedState {
    fn stack_pointer(&self) -> usize {
        self.sp as usize
    }

    /// Reads syscall argument register 0.
    fn arg0(&self) -> usize {
        self.a0 as usize
    }
    fn arg1(&self) -> usize {
        self.a1 as usize
    }
    fn arg2(&self) -> usize {
        self.a2 as usize
    }
    fn arg3(&self) -> usize {
        self.a3 as usize
    }
    fn arg4(&self) -> usize {
        self.a4 as usize
    }
    fn arg5(&self) -> usize {
        self.a5 as usize
    }
    fn arg6(&self) -> usize {
        self.a6 as usize
    }

    fn syscall_descriptor(&self) -> usize {
        self.a7 as usize
    }

    /// Writes syscall return argument 0.
    fn ret0(&mut self, x: usize) {
        self.a0 = x as u64
    }
    fn ret1(&mut self, x: usize) {
        self.a1 = x as u64
    }
    fn ret2(&mut self, x: usize) {
        self.a2 = x as u64
    }
    fn ret3(&mut self, x: usize) {
        self.a3 = x as u64
    }
    fn ret4(&mut self, x: usize) {
        self.a4 = x as u64
    }
    fn ret5(&mut self, x: usize) {
        self.a5 = x as u64
    }
}
