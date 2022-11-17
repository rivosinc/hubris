// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use anyhow::{bail, Result};
use std::path::Path;
use std::process::Command;
use std::string::String;

/// Rewrites a file in-place using rustfmt.
///
/// Rustfmt likes to rewrite files in-place. If this concerns you, copy your
/// important file to a temporary file, and then call this on it.
pub fn rustfmt(path: impl AsRef<Path>) -> Result<()> {
    let mut cmd =
        match Command::new("rustup").args(["which", "rustfmt"]).output() {
            Ok(out) => {
                if !out.status.success() {
                    bail!("rustup which returned status {}", out.status);
                } else {
                    Command::new(String::from_utf8(out.stdout)?.trim())
                }
            }
            Err(_) => match Command::new("rustfmt").output() {
                Ok(_) => Command::new("rustfmt"),
                Err(_) => bail!("No rustfmt available"),
            },
        };

    let fmt_status = cmd.arg(path.as_ref()).status()?;
    if !fmt_status.success() {
        bail!("rustfmt returned status {}", fmt_status);
    }
    Ok(())
}
