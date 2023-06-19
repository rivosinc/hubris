// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use std::collections::HashMap;
use std::fs::File;
use std::io::Write;

use abi::{
    InterruptDestination, InterruptNum, InterruptOwner, InterruptSource,
};
use anyhow::{bail, Context, Result};
use build_kconfig::{
    KernelConfig, OwnedAddress, RegionAttributes, RegionConfig, SpecialRole,
};
use indexmap::IndexMap;
use lib_util::Hex;
use proc_macro2::TokenStream;

fn main() -> Result<()> {
    build_util::expose_cpu_info()?;

    println!("cargo::rustc-check-cfg=cfg(hubris_phantom_svc_mitigation)");
    if build_util::target().starts_with("thumbv6m") {
        // Force SVC checks on for v6-M.
        println!("cargo:rustc-cfg=hubris_phantom_svc_mitigation");
    }

    let g = process_config()?;
    generate_statics(&g)?;

    Ok(())
}

struct Generated {
    tasks: Vec<TokenStream>,
    task_regions: Vec<TokenStream>,
    kernel_regions: Vec<TokenStream>,
    irq_code: TokenStream,
}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
enum RegionKey {
    Null,
    Shared(String),
    Owned {
        /// Index of the task, based on ordering in the kconfig
        task_index: usize,

        /// Index of this particular region within the task
        ///
        /// Each task's memory span can be built from multiple contiguous MPU
        /// regions; if that's the case, then `chunk_index` varies.  The region
        /// with `chunk_index == 0` is at the base address.
        chunk_index: usize,

        /// Name of memory which we're using for this region
        memory_name: String,
    },
}

fn process_config() -> Result<Generated> {
    let kconfig: KernelConfig =
        ron::de::from_str(&build_util::env_var("HUBRIS_KCONFIG")?)
            .context("parsing kconfig from HUBRIS_KCONFIG")?;

    // The kconfig data structure keeps things somewhat abstract to give us, the
    // kernel, more freedom about our internal implementation choices. However,
    // this means we have to do some preprocessing before it's useful.

    // The kernel currently uses a flat region descriptor table, and tasks get
    // pointers into that table. Let's assemble that flat table. By putting the
    // regions into an IndexMap, we make their locations findable (using a
    // RegionKey) while also creating a predictable ordering.

    let mut kernel_region_table: IndexMap<RegionKey, RegionConfig> =
        IndexMap::new();
    for (name, region) in &kconfig.kernel_regions {
        kernel_region_table.insert(
            RegionKey::Owned {
                task_index: 0,
                chunk_index: 0,
                memory_name: name.clone(),
            },
            *region,
        );
    }

    // We are done mutating this.
    let kernel_region_table = kernel_region_table;

    let kernel_region_descs = kernel_region_table
        .into_iter()
        .map(|(_k, region)| fmt_region(&region, true))
        .collect();

    let mut task_region_table = IndexMap::new();

    // We reserve the first entry (index 0) for the "null" region. This gives no
    // access, and plays the important role of intercepting null pointer
    // dereferences _in the kernel._
    task_region_table.insert(
        RegionKey::Null,
        RegionConfig {
            base: 0,
            size: 32,
            attributes: RegionAttributes {
                read: false,
                write: false,
                execute: false,
                special_role: None,
            },
        },
    );

    // We'll do the shared bits next, ignoring empty regions.
    for (name, region) in kconfig
        .shared_regions
        .iter()
        .filter(|(_, region)| region.size > 0)
    {
        task_region_table.insert(RegionKey::Shared(name.clone()), *region);
    }

    // Finally, the task-specific regions.
    for (i, task) in kconfig.tasks.iter().enumerate() {
        for (name, region) in &task.owned_regions {
            let mut base = region.base;
            for (j, &size) in region.sizes.iter().enumerate() {
                let r = RegionConfig {
                    base,
                    size,
                    attributes: region.attributes,
                };
                base += size;
                task_region_table.insert(
                    RegionKey::Owned {
                        task_index: i,
                        chunk_index: j,
                        memory_name: name.clone(),
                    },
                    r,
                );
            }
        }
    }

    // We are done mutating this.
    let task_region_table = task_region_table;

    // Now, generate the TaskDesc literals. These rely on the region table
    // because they address it by index at the moment.
    let mut task_descs = vec![];

    for (i, task) in kconfig.tasks.iter().enumerate() {
        // Work out the region indices for each of this task's regions.
        let mut regions = vec![
            // Always include the null region.
            task_region_table.get_index_of(&RegionKey::Null).unwrap(),
        ];

        for (name, region) in &task.owned_regions {
            for j in 0..region.sizes.len() {
                regions.push(
                    task_region_table
                        .get_index_of(&RegionKey::Owned {
                            task_index: i,
                            chunk_index: j,
                            memory_name: name.clone(),
                        })
                        .unwrap(),
                );
            }
        }

        for name in task
            .shared_regions
            .iter()
            .filter(|&name| kconfig.shared_regions[name].size > 0)
        {
            regions.push(
                task_region_table
                    .get_index_of(&RegionKey::Shared(name.clone()))
                    .with_context(|| {
                        format!("task {i} uses unknown device {name}")
                    })?,
            );
        }

        if regions.len() > 8 {
            bail!("too many regions ({}) for task {i}", regions.len());
        }
        regions.resize(8, 0usize);

        // Order the task's regions in ascending address order.
        //
        // THIS IS IMPORTANT. The kernel exploits this property to do cheaper
        // access tests.
        regions
            .sort_by_key(|i| task_region_table.get_index(*i).unwrap().1.base);

        // Translate abstract addresses in the task description into concrete
        // addresses.
        let entry_point =
            translate_address(&task_region_table, i, task.entry_point.clone());
        let initial_stack = translate_address(
            &task_region_table,
            i,
            task.initial_stack.clone(),
        );

        let index = u16::try_from(i).expect("over 2**16 tasks??");
        let priority = task.priority;
        let flags = if task.start_at_boot {
            quote::quote! { TaskFlags::START_AT_BOOT }
        } else {
            quote::quote! { TaskFlags::empty() }
        };

        // convert fields into hex values
        let entry_point = Hex(entry_point);
        let initial_stack = Hex(initial_stack);
        task_descs.push(quote::quote! {
            TaskDesc {
                regions: [#(&HUBRIS_REGION_DESCS[#regions]),*],
                entry_point: #entry_point,
                initial_stack: #initial_stack,
                priority: #priority,
                index: #index,
                flags: #flags,
            }
        });
    }

    let region_descs = task_region_table
        .into_iter()
        .map(|(_k, region)| fmt_region(&region, false))
        .collect();

    // Now, we generate two mappings and a list:
    //  InterruptSource => InterruptOwner
    //  InterruptOwner => InterruptSource
    //
    // The first table allows for efficient implementation of the default
    // interrupt handler, which needs to look up the destination of a
    // given interrupt.
    //
    // The second table allows for efficient implementation of `irq_control`,
    // where a task enables or disables one or more IRQS based on notification
    // masks.
    //
    // The form of the mapping will depend on the target architecture, below.
    //
    // The list contains the interrupts to enable by default.
    let irq_task_map = kconfig
        .irqs
        .iter()
        .map(|(&k, &v)| (k, v))
        .collect::<Vec<_>>();
    let ext_int_ctrl_num = kconfig.external_interrupt_controllers.len();
    let int_ctrl_num = ext_int_ctrl_num + 1; // add default interrupt controller

    let mut per_task_irqs: HashMap<_, Vec<_>> = HashMap::new();
    let mut default_interrupts = Vec::new();
    let mut int_ctrl_tree: HashMap<_, Vec<_>> = HashMap::new();
    for (irq, cfg) in &irq_task_map {
        match cfg {
            InterruptDestination::Owner(owner) => {
                per_task_irqs.entry(*owner).or_default().push(*irq)
            }
            // Interrupt controllers are not linked to a task
            InterruptDestination::Controller(ctrl) => {
                int_ctrl_tree.entry(irq.controller).or_default().push(*ctrl);
                default_interrupts.push(*irq)
            }
        }
    }
    let task_irq_map = per_task_irqs.into_iter().collect::<Vec<_>>();
    let mut irq_task_maps: Vec<Vec<(InterruptNum, InterruptDestination)>> =
        Vec::new();
    for i in 0..int_ctrl_num {
        // get interrupts linked to an interrupt controller
        let ctrl_irq_task_map = irq_task_map
            .iter()
            .filter(|(k, _)| k.controller == i as u32)
            .map(|(k, v)| (InterruptNum(k.irq), v.to_owned()))
            .collect();
        irq_task_maps.push(ctrl_irq_task_map);
    }
    let irq_task_maps = irq_task_maps;

    // build the default interrupt list
    let default_interrupts_num = default_interrupts.len();
    let default_interrupts: Vec<_> = default_interrupts
        .iter()
        .map(|InterruptSource { controller, irq }| {
            quote::quote! {
                InterruptSource { controller: #controller, irq: #irq }
            }
        })
        .collect();
    let default_interrupts = quote::quote! {
        pub const HUBRIS_DEFAULT_INTERRUPTS: [InterruptSource; #default_interrupts_num] = [
            #(#default_interrupts),*
        ];
    };

    let target = build_util::target();
    let irq_code = if target.starts_with("thumbv6m")
        || target.starts_with("riscv")
    {
        // On ARMv6-M we have no hardware division, which the perfect hash table
        // relies on (to get efficient integer remainder). Fall back to a good
        // old sorted list with binary search instead.
        //
        // This means our dispatch time for interrupts on ARMv6-M is O(log N)
        // instead of O(1), but these parts also tend to have few interrupts,
        // so, not the end of the world.

        // Generate text for task => irq, stored in the sorted lists
        let task_irq_map = phash_gen::OwnedSortedList::build(task_irq_map)
            .context("building task-to-IRQ map")?;
        let task_irq_literal = fmt_sorted_list(&task_irq_map, fmt_task_irq);

        // Generate text for irq => interrupt controller/task, stored in the sorted lists
        let mut irq_task_literals = Vec::new();
        for map in irq_task_maps.iter() {
            // get interrupts linked to an interrupt controller
            let irq_task_map = phash_gen::OwnedSortedList::build(map.clone())
                .context("building IRQ-to-task map")?;
            let irq_task_literal = fmt_sorted_list(&irq_task_map, fmt_irq_task);
            irq_task_literals.push(irq_task_literal);
        }

        quote::quote! {
            pub const HUBRIS_IRQ_TASK_LOOKUP:
                [phash::SortedList<'_, InterruptNum, InterruptDestination>; #int_ctrl_num]
                = [#(#irq_task_literals),*];
            pub const HUBRIS_TASK_IRQ_LOOKUP:
                phash::SortedList<'_,
                InterruptOwner,
                &'static [InterruptSource],
                > = #task_irq_literal;
            #default_interrupts
        }
    } else if target.starts_with("thumbv7m")
        || target.starts_with("thumbv7em")
        || target.starts_with("thumbv8m")
    {
        // First, try to build it as a single-level perfect hash map, which is
        // cheaper but won't always succeed.
        let map1 = if let Ok(task_irq_map) =
            phash_gen::OwnedPerfectHashMap::build(task_irq_map.clone())
        {
            let map_literal =
                fmt_perfect_hash_map(&task_irq_map, fmt_opt_task_irq);
            quote::quote! {
                pub const HUBRIS_TASK_IRQ_LOOKUP:
                    phash::PerfectHashMap<'_,
                    InterruptOwner,
                    &'static [InterruptSource],
                    > = #map_literal;
            }
        } else {
            // Single-level perfect hash failed, make it work with a nested map.
            let task_irq_map =
                phash_gen::OwnedNestedPerfectHashMap::build(task_irq_map)
                    .context("building task-to-IRQ perfect hash")?;
            let task_irq_literal =
                fmt_nested_perfect_hash_map(&task_irq_map, fmt_opt_task_irq);
            quote::quote! {
                pub const HUBRIS_TASK_IRQ_LOOKUP:
                    phash::NestedPerfectHashMap<'_,
                    InterruptOwner,
                    &'static [InterruptSource],
                    > = #task_irq_literal;
            }
        };

        // And now repeat the process for the IRQ-to-task direction.
        let map2 = 'map: {
            let mut map_literals = Vec::new();
            for map in irq_task_maps.iter() {
                if let Ok(irq_task_map) =
                    phash_gen::OwnedPerfectHashMap::build(map.clone())
                {
                    let map_literal =
                        fmt_perfect_hash_map(&irq_task_map, fmt_opt_irq_task);
                    map_literals.push(map_literal);
                } else {
                    map_literals.clear();
                    break;
                }
            }

            if !map_literals.is_empty() {
                // single level hash map succeeded
                break 'map quote::quote! {
                    pub const HUBRIS_IRQ_TASK_LOOKUP:
                        [phash::PerfectHashMap<
                        '_,
                        InterruptNum,
                        InterruptDestination,
                        >; #int_ctrl_num] = [#(#map_literals),*];
                };
            }

            // single level hash map failed
            for map in irq_task_maps.iter() {
                let irq_task_map =
                    phash_gen::OwnedNestedPerfectHashMap::build(map.clone())
                        .context("building IRQ-to-task perfect hash")?;
                let map_literal = fmt_nested_perfect_hash_map(
                    &irq_task_map,
                    fmt_opt_irq_task,
                );
                map_literals.push(map_literal);
            }

            quote::quote! {
                pub const HUBRIS_IRQ_TASK_LOOKUP:
                    [phash::NestedPerfectHashMap<
                    InterruptNum,
                    InterruptDestination,
                    >; #int_ctrl_num] = [#(#map_literals),*];
            }
        };

        quote::quote! {
            #map1
            #map2
            #default_interrupts
        }
    } else {
        panic!("Don't know the target {target}");
    };

    // compute the depth on the interrupt controller tree
    // Initial value is 1 as there is always the default interrupt controller
    let mut depth = 1_usize;
    if !int_ctrl_tree.is_empty() {
        // get children from the default interrupt controller
        let mut branches = int_ctrl_tree
            .get(&0)
            .context("Missing default interrupt controller")?
            .clone();
        loop {
            depth += 1;
            let mut branches_to_explore = Vec::new();
            for branch in branches {
                if let Some(next_branches) = int_ctrl_tree.get(&branch) {
                    branches_to_explore.extend(next_branches);
                }
            }
            if branches_to_explore.is_empty() {
                // we have explored all the levels of the tree
                break;
            }
            branches = branches_to_explore;
        }
    }

    let irq_code = quote::quote! {
        #irq_code

        #[allow(dead_code)]
        pub const MAX_NESTED_INTR_CTRL: usize = #depth;
    };

    Ok(Generated {
        tasks: task_descs,
        task_regions: region_descs,
        kernel_regions: kernel_region_descs,
        irq_code,
    })
}

fn translate_address(
    region_table: &IndexMap<RegionKey, RegionConfig>,
    task_index: usize,
    address: OwnedAddress,
) -> u32 {
    // Addresses within a particular task's memory span can be calculated from
    // the base address of the task's first memory region, which has a
    // chunk_index of 0 (since all chunks within the span are contiguous).
    let key = RegionKey::Owned {
        task_index,
        chunk_index: 0,
        memory_name: address.region_name,
    };
    region_table[&key].base + address.offset
}

fn fmt_region(region: &RegionConfig, is_kern_region: bool) -> TokenStream {
    let RegionConfig {
        base,
        size,
        attributes,
    } = region;

    let mut atts = vec![];
    if attributes.read {
        atts.push(quote::quote! { READ });
    }
    if attributes.write {
        atts.push(quote::quote! { WRITE });
    }
    if attributes.execute {
        atts.push(quote::quote! { EXECUTE });
    }
    if let Some(role) = attributes.special_role {
        atts.push(match role {
            SpecialRole::Device => quote::quote! { DEVICE },
            SpecialRole::Dma => quote::quote! { DMA },
        });
    }

    let atts = if atts.is_empty() {
        quote::quote! { RegionAttributes::empty() }
    } else {
        // We have to do the OR-ing on bits and then from_bits_unchecked it
        // because these operations are const, while OR-ing of the
        // RegionAttributes type itself is not (pending const impl stability)
        quote::quote! {
            unsafe {
                RegionAttributes::from_bits_unchecked(
                    #(RegionAttributes::#atts.bits())|*
                )
            }
        }
    };

    // convert fields into hex values
    let base = Hex(*base);
    let size = Hex(*size);

    quote::quote! {
        RegionDesc {
            base: #base,
            size: #size,
            attributes: #atts,
            arch_data: crate::arch::compute_region_extension_data(
                #base, #size, #atts, #is_kern_region
            ),
        }
    }
}

fn generate_statics(gen: &Generated) -> Result<()> {
    let image_id: u64 = build_util::env_var("HUBRIS_IMAGE_ID")?
        .parse()
        .context("parsing HUBRIS_IMAGE_ID")?;

    let out = build_util::out_dir();
    let kconfig_path = out.join("kconfig.rs");
    let mut file =
        File::create(&kconfig_path).context("creating kconfig.rs")?;

    writeln!(file, "// See build.rs for details")?;

    /////////////////////////////////////////////////////////
    // Basic constants and empty space

    let task_count = gen.tasks.len();
    writeln!(
        file,
        "{}",
        quote::quote! {
            const HUBRIS_TASK_COUNT: usize = #task_count;
            #[no_mangle]
            pub static HUBRIS_IMAGE_ID: u64 = #image_id;

            static mut HUBRIS_TASK_TABLE_SPACE:
                core::mem::MaybeUninit<[crate::task::Task; HUBRIS_TASK_COUNT]> =
                core::mem::MaybeUninit::uninit();
        },
    )?;

    /////////////////////////////////////////////////////////
    // Task descriptors

    let task_descs = &gen.tasks;
    writeln!(
        file,
        "{}",
        quote::quote! {
            static HUBRIS_TASK_DESCS: [TaskDesc; HUBRIS_TASK_COUNT] = [
                #(#task_descs,)*
            ];

        },
    )?;

    /////////////////////////////////////////////////////////
    // Region descriptors

    let regions = &gen.task_regions;
    let region_count = regions.len();
    writeln!(
        file,
        "{}",
        quote::quote! {
            static HUBRIS_REGION_DESCS: [RegionDesc; #region_count] = [
                #(#regions,)*
            ];
        },
    )?;

    /////////////////////////////////////////////////////////
    // Kernel Region descriptors

    let regions = &gen.kernel_regions;
    let region_count = regions.len();
    writeln!(
        file,
        "{}",
        quote::quote! {
            #[allow(dead_code)]
            static KERNEL_REGION_DESCS: [RegionDesc; #region_count] = [
                #(#regions,)*
            ];
        },
    )?;

    /////////////////////////////////////////////////////////
    // Interrupt table

    writeln!(file, "{}", gen.irq_code)?;

    drop(file);
    call_rustfmt::rustfmt(kconfig_path)?;

    Ok(())
}

fn fmt_opt_task_irq(
    v: Option<&(InterruptOwner, Vec<InterruptSource>)>,
) -> TokenStream {
    match v {
        Some((owner, irqs)) => fmt_task_irq(owner, irqs),
        None => quote::quote! {
            (InterruptOwner::invalid(), &[])
        },
    }
}

#[allow(clippy::ptr_arg)] // The function signature is enforced by its use.
fn fmt_task_irq(
    owner: &InterruptOwner,
    irqs: &Vec<InterruptSource>,
) -> TokenStream {
    let (task, not) = (owner.task, owner.notification);
    let ctrls: Vec<_> = irqs.iter().map(|t| t.controller).collect();
    let irq_nums: Vec<_> = irqs.iter().map(|t| t.irq).collect();
    quote::quote! {
        (
            InterruptOwner { task: #task, notification: #not },
            &[#(InterruptSource { controller: #ctrls, irq: #irq_nums }),*],
        )
    }
}

fn fmt_opt_irq_task(
    v: Option<&(InterruptNum, InterruptDestination)>,
) -> TokenStream {
    match v {
        Some((irq, owner)) => fmt_irq_task(irq, owner),
        None => quote::quote! { (
                InterruptNum::invalid(),
                InterruptDestination::Owner(InterruptOwner::invalid()),
                )
        },
    }
}

fn fmt_irq_task(
    irq: &InterruptNum,
    dest: &InterruptDestination,
) -> TokenStream {
    let irq = irq.0;
    match dest {
        InterruptDestination::Controller(ctrl) => {
            quote::quote! { (InterruptNum(#irq), InterruptDestination::Controller(#ctrl))}
        }
        InterruptDestination::Owner(InterruptOwner { task, notification }) => {
            quote::quote! { (
                    InterruptNum(#irq),
                    InterruptDestination::Owner(InterruptOwner {
                        task: #task, notification: #notification
                    }))
            }
        }
    }
}

fn fmt_sorted_list<K, V>(
    list: &phash_gen::OwnedSortedList<K, V>,
    element: impl Fn(&K, &V) -> TokenStream,
) -> TokenStream {
    let values = list.values.iter().map(|(k, v)| element(k, v));
    quote::quote! {
        phash::SortedList {
            values: &[#(#values,)*],
        }
    }
}

fn fmt_perfect_hash_map<K, V>(
    map: &phash_gen::OwnedPerfectHashMap<K, V>,
    element: impl Fn(Option<&(K, V)>) -> TokenStream,
) -> TokenStream {
    let values = map.values.iter().map(|o| element(o.as_ref()));
    let m = map.m;
    quote::quote! {
        phash::PerfectHashMap {
            m: #m,
            values: &[#(#values,)*],
        }
    }
}

fn fmt_nested_perfect_hash_map<K, V>(
    map: &phash_gen::OwnedNestedPerfectHashMap<K, V>,
    element: impl Fn(Option<&(K, V)>) -> TokenStream,
) -> TokenStream {
    let values = map.values.iter().map(|v| {
        let inner = v.iter().map(|o| element(o.as_ref()));
        quote::quote! {
            &[#(#inner,)*]
        }
    });
    let m = map.m;
    let g = &map.g;
    quote::quote! {
        phash::NestedPerfectHashMap {
            m: #m,
            g: &[#(#g,)*],
            values: &[#(#values,)*],
        }
    }
}
