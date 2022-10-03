// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

task_config::task_config! {
    #[allow(dead_code)]
    clock_frequency_hz: u32,
    bark_threshold_ticks: u64,
    bite_threshold_ticks: u64,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    assert!(
        TASK_CONFIG.bark_threshold_ticks < TASK_CONFIG.bite_threshold_ticks,
        "Bark threshold must be less than the bite threshold"
    );
    Ok(())
}
