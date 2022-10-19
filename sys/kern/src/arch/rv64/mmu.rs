// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use crate::task;
use riscv::register;

pub fn apply_memory_protection(task: &task::Task) {
    // TODO: Apply protection via S-mode accessible instructions
}
