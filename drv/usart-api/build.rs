// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use anyhow::{anyhow, Result};

fn main() -> Result<()> {
    idol::Generator::new()
        .with_op_enum_derives(std::iter::once("counters::Count"))?
        .build_client_stub("../../idl/usart.idol", "client_stub.rs")
        .map_err(|e| anyhow!(e))?;

        Ok(())
}
