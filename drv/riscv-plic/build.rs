// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use std::collections::HashMap;
use std::env;
use std::fs::File;
use std::io::Write;
use std::path::PathBuf;

task_config::task_config! {
    ints: &'static [u16],
    tasks: &'static [&'static str],
    notification: &'static [u32],
    base: u32,
    pbits: u8,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    build_util::expose_target_board();

    idol::server::build_server_support(
        "../../idl/riscv-plic.idol",
        "server_stub.rs",
        idol::server::ServerStyle::InOrder,
    )?;

    let out = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    let mut file = File::create(out.join("plic_config.rs")).unwrap();

    writeln!(
        file,
        "use phash::{{PerfectHashMap, MutablePerfectHashMap}};"
    )?;

    writeln!(
        file,
        "const PLIC_PRIORITY_BITS: usize = 0x{:X};",
        TASK_CONFIG.pbits
    )
    .unwrap();
    writeln!(file, "const PLIC_REGISTER_BLOCK: *mut Plic<PLIC_PRIORITY_BITS> = 0x{:X} as *mut Plic<PLIC_PRIORITY_BITS>;", TASK_CONFIG.base).unwrap();
    writeln!(file, "type Priority = plic::Priority<PLIC_PRIORITY_BITS>;")
        .unwrap();

    /*
    let mut task_set: HashSet<String> = HashSet::new();
    for task in TASK_CONFIG.tasks.iter() {
        if task_set.insert(task.to_string()) == true {
            writeln!(file, "task_slot!({}, {});", task.to_ascii_uppercase(), task);
    }
    */

    use abi::{InterruptNum, InterruptOwner, TaskId};
    let fmt_irq_task = |v: Option<&(InterruptNum, (TaskId, u32))>| {
        match v {
            Some((irq, owner)) => format!(
                "(userlib::InterruptNum({}), (TaskId({}), 0b{:b})),",
                irq.0, owner.0.0, owner.1
            ),
            None => "(userlib::InterruptNum::invalid(), userlib::InterruptOwner::invalid()),"
                .to_string(),
        }
    };

    let fmt_task_irq = |v: Option<&(InterruptOwner, Vec<InterruptNum>)>| {
        match v {
            Some((owner, irqs)) => format!(
                "(userlib::InterruptOwner {{ task: {}, notification: 0b{:b} }}, &[{}]),",
                owner.task, owner.notification,
                irqs.iter()
                    .map(|i| format!("userlib::InterruptNum({})", i.0))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            None => {
                "(TaskId::IndexMask - 1, &[]),"
                    .to_string()
            }
        }
    };

    println!("{}", &env::var("HUBRIS_TASK_ID_MAP")?);
    let task_id_map: std::collections::BTreeMap<String, u16> =
        ron::de::from_str(&env::var("HUBRIS_TASK_ID_MAP")?)?;

    let mut irq_task_map: Vec<(InterruptNum, (TaskId, u32))> = Vec::new();
    for (i, irq) in TASK_CONFIG.ints.into_iter().enumerate() {
        let task: String = TASK_CONFIG.tasks[i].to_string();
        let task_id: TaskId = match task_id_map.get(&task) {
            Some(id_num) => TaskId(*id_num),
            None => panic!("Error: no matching task ID for task {}", task),
        };
        let notification: u32 = TASK_CONFIG.notification[i];
        irq_task_map.push((InterruptNum(*irq as u32), (task_id, notification)));
    }

    let mut per_task_irqs: HashMap<InterruptOwner, Vec<InterruptNum>> =
        HashMap::new();
    for (i, task) in TASK_CONFIG.tasks.iter().enumerate() {
        let notification: u32 = TASK_CONFIG.notification[i];
        let irq: InterruptNum = InterruptNum(TASK_CONFIG.ints[i] as u32);
        let task: String = task.to_string();
        let task_id: TaskId = match task_id_map.get(&task) {
            Some(id_num) => TaskId(*id_num),
            None => panic!("Error: no matching task ID for task {}", task),
        };
        let owner: InterruptOwner = InterruptOwner {
            task: task_id.index() as u32,
            notification: notification,
        };
        per_task_irqs.entry(owner).or_default().push(irq);
    }
    let task_irq_map: Vec<(InterruptOwner, Vec<InterruptNum>)> =
        per_task_irqs.into_iter().collect::<Vec<_>>();

    if let Ok(irq_task_map) =
        phash_gen::OwnedPerfectHashMap::build(irq_task_map.clone())
    {
        // Generate text for the Interrupt and InterruptSet tables stored in the
        // PerfectHashes
        let irq_task_value = irq_task_map
            .values
            .iter()
            .map(|o| fmt_irq_task(o.as_ref()))
            .collect::<Vec<String>>()
            .join("\n        ");

        writeln!(file, "
static mut HUBRIS_IRQ_TASK_LOOKUP: MutablePerfectHashMap::<userlib::InterruptNum, (TaskId, u32)> = MutablePerfectHashMap {{
m: {:#x},
values: &mut [
    {}
],
}};",
            irq_task_map.m, irq_task_value)?;
    } else {
        panic!("Can't make HUBRIS_IRQ_TASK_LOOKUP");
    }

    if let Ok(task_irq_map) =
        phash_gen::OwnedPerfectHashMap::build(task_irq_map.clone())
    {
        let task_irq_value = task_irq_map
            .values
            .iter()
            .map(|o| fmt_task_irq(o.as_ref()))
            .collect::<Vec<String>>()
            .join("\n        ");
        writeln!(file, "
pub const HUBRIS_TASK_IRQ_LOOKUP: PerfectHashMap::<userlib::InterruptOwner, &'static [userlib::InterruptNum]> = PerfectHashMap {{
m: {:#x},
values: &[
    {}
],
}};",
            task_irq_map.m, task_irq_value)?;
    } else {
        panic!("Can't make HUBRIS_TASK_IRQ_LOOKUP");
    }

    Ok(())
}
