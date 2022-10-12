// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};
use std::fs::{self, File};
use std::hash::{Hash, Hasher};
use std::io::Write;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::process::Command;

use abi::AbiSize;
use anyhow::{anyhow, bail, Context, Result};
use atty::Stream;
use colored::*;
use indexmap::IndexMap;
use paste::paste;
use path_slash::PathBufExt;
use serde::Serialize;
use zerocopy::AsBytes;

use crate::{
    config::{BuildConfig, Config},
    elf,
    sizes::load_task_size,
    task_slot,
};

/// In practice, applications with active interrupt activity tend to use about
/// 650 bytes of stack. Because kernel stack overflows are annoying, we've
/// padded that a bit.
pub const DEFAULT_KERNEL_STACK: AbiSize = 1024;

#[derive(Copy, Clone)]
enum ArchTarget {
    ARM,
    RISCV32,
    RISCV64,
}

struct ArchConsts<'a> {
    /// Objcopy string
    objcopy_cmd: &'a str,

    /// Objcopy target format
    objcopy_target: &'a str,

    /// Link script to use
    link_script: &'a str,

    /// Kernel link script to use
    kernel_link_script: &'a str,

    /// Relocatable linker script
    rlink_script: &'a str,

    /// Temporary linker script
    tlink_script: &'a str,
}

const ARM_CONSTS: ArchConsts<'static> = ArchConsts {
    objcopy_cmd: "arm-none-eabi-objcopy",
    objcopy_target: "elf32-littlearm",
    link_script: "lds/arm/task-link.x",
    kernel_link_script: "lds/arm/kernel-link.x",
    rlink_script: "lds/arm/task-rlink.x",
    tlink_script: "lds/arm/task-tlink.x",
};

const RISCV32_CONSTS: ArchConsts<'static> = ArchConsts {
    objcopy_cmd: "riscv64-unknown-elf-objcopy",
    objcopy_target: "elf32-littleriscv",
    link_script: "lds/rv32/task-link.x",
    kernel_link_script: "lds/rv32/kernel-link.x",
    rlink_script: "lds/rv32/task-rlink.x",
    // riscv-task-link.x doesn't do flash fill currently, so there's no point
    // in having a separate linker script.
    tlink_script: "lds/rv32/task-tlink.x",
};

const RISCV64_CONSTS: ArchConsts<'static> = ArchConsts {
    objcopy_cmd: "riscv64-unknown-elf-objcopy",
    objcopy_target: "elf64-littleriscv",
    link_script: "lds/rv64/task-link.x",
    kernel_link_script: "lds/rv64/kernel-link.x",
    rlink_script: "lds/rv64/task-rlink.x",
    // riscv-task-link.x doesn't do flash fill currently, so there's no point
    // in having a separate linker script.
    tlink_script: "lds/rv64/task-tlink.x",
};

/// `PackageConfig` contains a bundle of data that's commonly used when
/// building a full app image, grouped together to avoid passing a bunch
/// of individual arguments to functions.
///
/// It should be trivial to calculate and kept constant during the build;
/// mutable build information should be accumulated elsewhere.
struct PackageConfig<'a> {
    /// Path to the `app.toml` file being built
    app_toml_file: PathBuf,

    /// Directory containing the `app.toml` file being built
    app_src_dir: PathBuf,

    /// Loaded configuration
    toml: Config,

    /// Add `-v` to various build commands
    verbose: bool,

    /// Run `cargo tree --edges` before compiling, to show dependencies
    edges: bool,

    /// Directory where the build artifacts are placed, in the form
    /// `target/$NAME/dist`.
    dist_dir: PathBuf,

    /// Sysroot of the relevant toolchain
    sysroot: PathBuf,

    /// Host triple, e.g. `aarch64-apple-darwin`
    host_triple: String,

    /// List of paths to be remapped by the compiler, to minimize strings in
    /// the resulting binaries.
    remap_paths: BTreeMap<PathBuf, &'static str>,

    /// A single value produced by hashing the various linker scripts. This
    /// allows us to force a rebuild when the linker scripts change, which
    /// is not normally tracked by `cargo build`.
    link_script_hash: u64,

    /// Target architecture
    arch_target: ArchTarget,

    /// Architecture-specific constants
    arch_consts: ArchConsts<'a>,
}

impl PackageConfig<'_> {
    fn new(app_toml_file: &Path, verbose: bool, edges: bool) -> Result<Self> {
        let toml = Config::from_file(app_toml_file)?;
        let dist_dir = Path::new("target").join(&toml.name).join("dist");
        let app_src_dir = app_toml_file
            .parent()
            .ok_or_else(|| anyhow!("Could not get app toml directory"))?;

        let sysroot = Command::new("rustc")
            .arg("--print")
            .arg("sysroot")
            .output()?;
        if !sysroot.status.success() {
            bail!("Could not find execute rustc to get sysroot");
        }
        let sysroot =
            PathBuf::from(std::str::from_utf8(&sysroot.stdout)?.trim());

        let host = Command::new(sysroot.join("bin").join("rustc"))
            .arg("-vV")
            .output()?;
        if !host.status.success() {
            bail!("Could not execute rustc to get host");
        }
        let host_triple = std::str::from_utf8(&host.stdout)?
            .lines()
            .find_map(|line| line.strip_prefix("host: "))
            .ok_or_else(|| anyhow!("Could not get host from rustc"))?
            .to_string();

        // xtask is built for the host system so we need to go with the target
        // specified in the app toml to decide which binutils and support files
        // to use
        let arch_target = if toml.target.starts_with("thumb") {
            ArchTarget::ARM
        } else if toml.target.starts_with("riscv32") {
            ArchTarget::RISCV32
        } else if toml.target.starts_with("riscv64") {
            ArchTarget::RISCV64
        } else {
            bail!("unsupported target");
        };

        let arch_consts = match arch_target {
            ArchTarget::ARM => ARM_CONSTS,
            ArchTarget::RISCV32 => RISCV32_CONSTS,
            ArchTarget::RISCV64 => RISCV64_CONSTS,
        };

        let mut extra_hash = fnv::FnvHasher::default();
        for f in [
            arch_consts.link_script,
            arch_consts.rlink_script,
            arch_consts.kernel_link_script,
        ] {
            let file_data = std::fs::read(Path::new(f))?;
            file_data.hash(&mut extra_hash);
        }

        Ok(Self {
            app_toml_file: app_toml_file.to_path_buf(),
            app_src_dir: app_src_dir.to_path_buf(),
            toml,
            verbose,
            edges,
            dist_dir,
            sysroot,
            host_triple,
            remap_paths: Self::remap_paths()?,
            link_script_hash: extra_hash.finish(),
            arch_target,
            arch_consts,
        })
    }

    fn img_dir(&self, img_name: &str) -> PathBuf {
        self.dist_dir.join(img_name)
    }

    fn img_file(&self, name: impl AsRef<Path>, img_name: &str) -> PathBuf {
        self.img_dir(img_name).join(name)
    }

    fn dist_file(&self, name: impl AsRef<Path>) -> PathBuf {
        self.dist_dir.join(name)
    }

    fn remap_paths() -> Result<BTreeMap<PathBuf, &'static str>> {
        // Panic messages in crates have a long prefix; we'll shorten it using
        // the --remap-path-prefix argument to reduce message size.  We'll remap
        // local (Hubris) crates to /hubris, crates.io to /crates.io, and git
        // dependencies to /git
        let mut remap_paths = BTreeMap::new();

        // On Windows, std::fs::canonicalize returns a UNC path, i.e. one
        // beginning with "\\hostname\".  However, rustc expects a non-UNC
        // path for its --remap-path-prefix argument, so we use
        // `dunce::canonicalize` instead
        let cargo_home = dunce::canonicalize(std::env::var("CARGO_HOME")?)?;
        let cargo_git = cargo_home.join("git").join("checkouts");
        remap_paths.insert(cargo_git, "/git");

        // This hash is canonical-ish: Cargo tries hard not to change it
        // https://github.com/rust-lang/cargo/blob/master/src/cargo/core/source/source_id.rs#L607-L630
        //
        // It depends on system architecture, so this won't work on (for example)
        // a Raspberry Pi, but the only downside is that panic messages will
        // be longer.
        let cargo_registry = cargo_home
            .join("registry")
            .join("src")
            .join("github.com-1ecc6299db9ec823");
        remap_paths.insert(cargo_registry, "/crates.io");

        let mut hubris_dir =
            dunce::canonicalize(std::env::var("CARGO_MANIFEST_DIR")?)?;
        hubris_dir.pop();
        hubris_dir.pop();
        remap_paths.insert(hubris_dir.to_path_buf(), "/hubris");
        Ok(remap_paths)
    }
}

pub fn list_tasks(app_toml: &Path) -> Result<()> {
    let toml = Config::from_file(app_toml)?;
    let pad = toml
        .tasks
        .keys()
        .map(String::as_str)
        .chain(std::iter::once("kernel"))
        .map(|m| m.len())
        .max()
        .unwrap_or(1);
    println!("  {:<pad$}  CRATE", "TASK", pad = pad);
    println!("  {:<pad$}  {}", "kernel", toml.kernel.name, pad = pad);
    for (name, task) in toml.tasks {
        println!("  {:<pad$}  {}", name, task.name, pad = pad);
    }
    Ok(())
}

#[derive(Debug)]
pub struct SecureData {
    secure: Range<AbiSize>,
    nsc: Range<AbiSize>,
}

/// Represents allocations and free spaces for a particular image
type AllocationMap = (Allocations, IndexMap<String, Range<AbiSize>>);

pub fn package(
    verbose: bool,
    edges: bool,
    app_toml: &Path,
    tasks_to_build: Option<Vec<String>>,
    dirty_ok: bool,
) -> Result<BTreeMap<String, AllocationMap>> {
    let cfg = PackageConfig::new(app_toml, verbose, edges)?;

    // If we're using filters, we change behavior at the end. Record this in a
    // convenient flag, running other checks as well.
    let (partial_build, tasks_to_build): (bool, BTreeSet<&str>) =
        if let Some(task_names) = tasks_to_build.as_ref() {
            check_task_names(&cfg.toml, task_names)?;
            (true, task_names.iter().map(|p| p.as_str()).collect())
        } else {
            assert!(!cfg.toml.tasks.contains_key("kernel"));
            check_task_priorities(&cfg.toml)?;
            (
                false,
                cfg.toml
                    .tasks
                    .keys()
                    .map(|p| p.as_str())
                    .chain(std::iter::once("kernel"))
                    .collect(),
            )
        };

    std::fs::create_dir_all(&cfg.dist_dir)?;
    if dirty_ok {
        println!("note: not doing a clean build because you asked for it");
    } else {
        check_rebuild(&cfg.toml)?;
    }

    // Build all tasks (which are relocatable executables, so they are not
    // statically linked yet). For now, we build them one by one and ignore the
    // return value, because we're going to link them regardless of whether the
    // build changed.
    for name in cfg.toml.tasks.keys() {
        if tasks_to_build.contains(name.as_str()) {
            build_task(&cfg, name)?;
        }
    }

    // Calculate the sizes of tasks, assigning dummy sizes to tasks that
    // aren't active in this build.
    let task_sizes: HashMap<_, _> = cfg
        .toml
        .tasks
        .keys()
        .map(|name| {
            let size = if tasks_to_build.contains(name.as_str()) {
                link_dummy_task(&cfg, name)?;
                task_size(&cfg, name)
            } else {
                // Dummy allocations
                let out: IndexMap<_, _> =
                    [("flash", 64), ("ram", 64)].into_iter().collect();
                Ok(out)
            };
            size.map(|sz| (name.as_str(), sz))
        })
        .collect::<Result<_, _>>()?;

    // Allocate memories.
    let allocated = allocate_all(&cfg.toml, &task_sizes)?;

    for image_name in &cfg.toml.image_names {
        // Build each task.
        let mut all_output_sections = BTreeMap::default();

        std::fs::create_dir_all(&cfg.img_dir(image_name))?;
        let (allocs, memories) = allocated
            .get(image_name)
            .ok_or_else(|| anyhow!("failed to get image name"))?;
        // Build all relevant tasks, collecting entry points into a HashMap.  If
        // we're doing a partial build, then assign a dummy entry point into
        // the HashMap, because the kernel kconfig will still need it.
        let entry_points: HashMap<_, _> = cfg
            .toml
            .tasks
            .keys()
            .map(|name| {
                let ep = if tasks_to_build.contains(name.as_str()) {
                    // Link tasks regardless of whether they have changed, because
                    // we don't want to track changes in the other linker input
                    // (task-link.x, memory.x, table.ld, etc)
                    link_task(&cfg, name, image_name, allocs)?;
                    task_entry_point(
                        &cfg,
                        name,
                        image_name,
                        &mut all_output_sections,
                    )
                } else {
                    // Dummy entry point
                    Ok(allocs.tasks[name]["flash"].start)
                };
                ep.map(|ep| (name.clone(), ep))
            })
            .collect::<Result<_, _>>()?;

        let s =
            secure_update(&cfg, allocs, &mut all_output_sections, image_name)?;

        // Build the kernel!
        let kern_build = if tasks_to_build.contains("kernel") {
            Some(build_kernel(
                &cfg,
                allocs,
                &mut all_output_sections,
                &cfg.toml.memories(image_name)?,
                &entry_points,
                image_name,
                &s,
            )?)
        } else {
            None
        };

        // If we've done a partial build (which may have included the kernel), bail
        // out here before linking stuff.
        if partial_build {
            return Ok(allocated);
        }

        // Print stats on memory usage
        let starting_memories = cfg.toml.memories(image_name)?;
        for (name, range) in &starting_memories {
            println!(
                "{:<5} = {:#010x}..{:#010x}",
                name, range.start, range.end
            );
        }
        println!("Used:");
        for (name, new_range) in memories {
            let orig_range = &starting_memories[name];
            let size = new_range.start - orig_range.start;
            let percent = size * 100 / (orig_range.end - orig_range.start);
            println!(
                "  {:<6} {:#x} ({}%)",
                format!("{}:", name),
                size,
                percent
            );
        }

        // Generate combined ELF, which is our source of truth for combined images.
        let (kentry, _ksymbol_table) = kern_build.unwrap();
        write_elf(
            &all_output_sections,
            kentry,
            &cfg,
            &cfg.img_file("combined.elf", image_name),
        )?;

        translate_elf_to_other_formats(&cfg, image_name, "combined")?;

        if let Some(signing) = &cfg.toml.signing {
            let priv_key = &signing.priv_key;
            let root_cert = &signing.root_cert;
            let rkth = lpc55_sign::signed_image::sign_image(
                &cfg.img_file("combined.bin", image_name),
                &cfg.app_src_dir.join(&priv_key),
                &cfg.app_src_dir.join(&root_cert),
                &cfg.img_file("combined_rsa.bin", image_name),
            )?;
            std::fs::copy(
                cfg.img_file("combined.bin", image_name),
                cfg.img_file("combined_original.bin", image_name),
            )?;
            std::fs::copy(
                cfg.img_file("combined_rsa.bin", image_name),
                cfg.img_file("combined.bin", image_name),
            )?;

            // We have to cheat a little for (re) generating the
            // srec after signing. The assumption is the binary starts
            // at the beginning of flash.
            binary_to_srec(
                &cfg.img_file("combined.bin", image_name),
                cfg.toml
                    .memories(image_name)?
                    .get(&"flash".to_string())
                    .ok_or_else(|| anyhow!("failed to get flash region"))?
                    .start,
                kentry,
                &cfg.img_file("final.srec", image_name),
            )?;

            translate_srec_to_other_formats(&cfg, image_name, "final")?;

            // The 'enable-dice' key causes the build to create a CMPA image
            // with DICE enabled, however the CFPA & keystore must be setup too
            // before the UDS can be created. See lpc55_support for details.
            lpc55_sign::signed_image::create_cmpa(
                signing.enable_dice,
                signing.dice_inc_nxp_cfg,
                signing.dice_cust_cfg,
                signing.dice_inc_sec_epoch,
                &rkth,
                &cfg.img_file("CMPA.bin", image_name),
            )?;
        } else {
            // If there's no bootloader, the "combined" and "final" images are
            // identical, so we copy from one to the other
            for ext in ["srec", "elf", "ihex", "bin"] {
                let src = format!("combined.{}", ext);
                let dst = format!("final.{}", ext);
                std::fs::copy(
                    cfg.img_file(src, image_name),
                    cfg.img_file(dst, image_name),
                )?;
            }
        }
        write_gdb_script(&cfg, image_name)?;
        build_archive(&cfg, image_name)?;
    }
    Ok(allocated)
}

fn secure_update(
    cfg: &PackageConfig,
    allocs: &Allocations,
    all_output_sections: &mut BTreeMap<AbiSize, LoadSegment>,
    image_name: &str,
) -> Result<Option<SecureData>> {
    if let Some(secure) = &cfg.toml.secure_task {
        if !cfg.toml.tasks.contains_key(secure) {
            bail!("secure task named {} not found!", secure);
        }
        // The secure task is our designated TrustZone region. We expect
        // this to have a non-secure callable (NSC) region for entry
        // pointers and a .tz_table of entry points
        let secure_bin = std::fs::read(&cfg.img_file(&secure, image_name))?;
        let secure_elf = goblin::elf::Elf::parse(&secure_bin)?;

        let nsc = match elf::get_section_by_name(&secure_elf, ".nsc") {
            Some(s) => s,
            _ => bail!("Couldn't find the nsc region in the secure task"),
        };

        if nsc.sh_size == 0 {
            bail!("nsc region is zero?");
        }

        let tz_table = match elf::get_section_by_name(&secure_elf, ".tz_table")
        {
            Some(s) => s,
            _ => bail!("Couldn't find the TZ table in the secure task"),
        };

        if tz_table.sh_size == 0 {
            bail!("tz_table is zero. This does not seem correct.");
        }

        let flash = &allocs.tasks[secure]["flash"];

        for (name, t) in &cfg.toml.tasks {
            // Any task listed as using secure needs to have an appropriately
            // sized .tz_table section which will get updated
            if t.uses_secure_entry {
                if t.name == *secure {
                    bail!("Secure task is selecting the secure region! This is wrong!");
                }

                let mut bin = std::fs::read(&cfg.img_file(name, image_name))?;
                let elf = goblin::elf::Elf::parse(&bin)?;

                let s = match elf::get_section_by_name(&elf, ".tz_table") {
                    Some(s) => s,
                    _ => bail!("task {} wants to use the secure region but doesn't have a slot for the TZ table", name),
                };

                if s.sh_size != tz_table.sh_size {
                    bail!("task {} has table size {:x} but secure table size is {:x}",
                            name, s.sh_size, tz_table.sh_size);
                }

                let target_start = s.sh_offset as usize;
                let target_end = (s.sh_offset + s.sh_size) as usize;

                let table_start = tz_table.sh_offset as usize;
                let table_end =
                    (tz_table.sh_offset + tz_table.sh_size) as usize;

                bin[target_start..target_end]
                    .clone_from_slice(&secure_bin[table_start..table_end]);

                std::fs::write(
                    &cfg.img_file(format!("{}.modified", name), image_name),
                    &bin,
                )?;
                std::fs::copy(
                    &cfg.img_file(format!("{}.modified", name), image_name),
                    &cfg.img_file(name, image_name),
                )?;

                let mut symbol_table = BTreeMap::default();
                let _ = load_elf(
                    &cfg.img_file(name, image_name),
                    all_output_sections,
                    &mut symbol_table,
                )?;
            }
        }

        let start = nsc.sh_addr as AbiSize;
        let end = (nsc.sh_addr + nsc.sh_size) as AbiSize;

        Ok(Some(SecureData {
            secure: flash.start..flash.end,
            nsc: start..end,
        }))
    } else {
        if cfg
            .toml
            .tasks
            .iter()
            .any(|(_, task)| task.uses_secure_entry)
        {
            bail!("task is using secure entry but no secure task is found!");
        }
        Ok(None)
    }
}

/// Convert SREC to other formats for convenience. Used in the signing flow.
fn translate_srec_to_other_formats(
    cfg: &PackageConfig,
    image_name: &str,
    name: &str,
) -> Result<()> {
    let src = cfg.img_dir(image_name).join(format!("{}.srec", name));
    for (out_type, ext) in [("ihex", "ihex"), ("binary", "bin")] {
        objcopy_translate_format(
            &cfg.arch_consts.objcopy_cmd,
            "srec",
            &src,
            out_type,
            &cfg.img_dir(image_name).join(format!("{}.{}", name, ext)),
        )?;
    }

    Ok(())
}

/// Convert ELF to other formats for convenience.
fn translate_elf_to_other_formats(
    cfg: &PackageConfig,
    image_name: &str,
    name: &str,
) -> Result<()> {
    let src = cfg.img_dir(image_name).join(format!("{}.elf", name));
    for (out_type, ext) in
        [("ihex", "ihex"), ("binary", "bin"), ("srec", "srec")]
    {
        objcopy_translate_format(
            &cfg.arch_consts.objcopy_cmd,
            &cfg.arch_consts.objcopy_target,
            &src,
            out_type,
            &cfg.img_dir(image_name).join(format!("{}.{}", name, ext)),
        )?;
    }

    Ok(())
}

fn write_gdb_script(cfg: &PackageConfig, image_name: &str) -> Result<()> {
    // Humility doesn't know about images right now. The gdb symbol file
    // paths all assume a flat layout with everything in dist. For now,
    // match what humility expects. If a build file ever contains multiple
    // images this will need to be fixed!
    let mut gdb_script = File::create(cfg.img_file("script.gdb", image_name))?;
    writeln!(
        gdb_script,
        "add-symbol-file {}",
        cfg.dist_file("kernel").to_slash().unwrap()
    )?;
    for name in cfg.toml.tasks.keys() {
        writeln!(
            gdb_script,
            "add-symbol-file {}",
            cfg.dist_file(name).to_slash().unwrap()
        )?;
    }
    for (path, remap) in &cfg.remap_paths {
        let mut path_str = path
            .to_str()
            .ok_or_else(|| anyhow!("Could not convert path{:?} to str", path))?
            .to_string();

        // Even on Windows, GDB expects path components to be separated by '/',
        // so we tweak the path here so that remapping works.
        if cfg!(windows) {
            path_str = path_str.replace('\\', "/");
        }
        writeln!(gdb_script, "set substitute-path {} {}", remap, path_str)?;
    }
    Ok(())
}

fn build_archive(cfg: &PackageConfig, image_name: &str) -> Result<()> {
    // Bundle everything up into an archive.
    let mut archive = Archive::new(
        cfg.img_file(format!("build-{}.zip", cfg.toml.name), image_name),
    )?;

    archive.text(
        "README.TXT",
        "\
        This is a build archive containing firmware build artifacts.\n\n\
        - app.toml is the config file used to build the firmware.\n\
        - git-rev is the commit it was built from, with optional dirty flag.\n\
        - info/ contains human-readable data like logs.\n\
        - elf/ contains ELF images for all firmware components.\n\
        - elf/tasks/ contains each task by name.\n\
        - elf/kernel is the kernel.\n\
        - img/ contains the final firmware images.\n\
        - debug/ contains OpenOCD and GDB scripts, if available.\n",
    )?;

    let (git_rev, git_dirty) = get_git_status()?;
    archive.text(
        "git-rev",
        format!("{}{}", git_rev, if git_dirty { "-dirty" } else { "" }),
    )?;
    archive.copy(&cfg.app_toml_file, "app.toml")?;
    let chip_dir = cfg.app_src_dir.join(cfg.toml.chip.clone());
    let chip_file = chip_dir.join("chip.toml");
    let chip_filename = chip_file.file_name().unwrap();
    archive.copy(&chip_file, &chip_filename)?;

    let elf_dir = PathBuf::from("elf");
    let tasks_dir = elf_dir.join("task");
    for name in cfg.toml.tasks.keys() {
        archive.copy(cfg.img_file(name, image_name), tasks_dir.join(name))?;
    }
    archive.copy(cfg.img_file("kernel", image_name), elf_dir.join("kernel"))?;

    let img_dir = PathBuf::from("img");

    for f in ["combined", "final"] {
        for ext in ["srec", "elf", "ihex", "bin"] {
            let name = format!("{}.{}", f, ext);
            archive
                .copy(cfg.img_file(&name, image_name), img_dir.join(&name))?;
        }
    }

    //
    // To allow for the image to be flashed based only on the archive (e.g.,
    // by Humility), we pull in our flash configuration, flatten it to pull in
    // any external configuration files, serialize it, and add it to the
    // archive.
    //
    if let Some(mut config) =
        crate::flash::config(cfg.toml.board.as_str(), &chip_dir)?
    {
        config.flatten()?;
        archive.text(
            img_dir.join("flash.ron"),
            ron::ser::to_string_pretty(
                &config,
                ron::ser::PrettyConfig::default(),
            )?,
        )?;
    }

    let debug_dir = PathBuf::from("debug");
    archive.copy(
        cfg.img_file("script.gdb", image_name),
        debug_dir.join("script.gdb"),
    )?;

    if let Some(auxflash) = cfg.toml.auxflash.as_ref() {
        let file = cfg.dist_file("auxi.tlvc");
        std::fs::write(&file, &auxflash.data)
            .context(format!("Failed to write auxi to {:?}", file))?;
        archive.copy(cfg.dist_file("auxi.tlvc"), img_dir.join("auxi.tlvc"))?;
    }

    // Copy `openocd.cfg` into the archive if it exists; it's not used for
    // the LPC55 boards.
    let openocd_cfg = chip_dir.join("openocd.cfg");
    if openocd_cfg.exists() {
        archive.copy(openocd_cfg, debug_dir.join("openocd.cfg"))?;
    }
    archive
        .copy(chip_dir.join("openocd.gdb"), debug_dir.join("openocd.gdb"))?;

    // Copy `qemu.sh` into the archive if it exists;
    // not all target may support qemu
    let qemu_sh = chip_dir.join("qemu.sh");
    if qemu_sh.exists() {
        archive.copy(qemu_sh, debug_dir.join("qemu.sh"))?;
    }

    archive.finish()?;
    Ok(())
}

fn check_task_names(toml: &Config, task_names: &[String]) -> Result<()> {
    // Quick sanity-check if we're trying to build individual tasks which
    // aren't present in the app.toml, or ran `cargo xtask build ...` without
    // any specified tasks.
    if task_names.is_empty() {
        bail!(
            "Running `cargo xtask build` without specifying tasks has no effect.\n\
             Did you mean to run `cargo xtask dist`?"
        );
    }
    let all_tasks = toml.tasks.keys().collect::<BTreeSet<_>>();
    if let Some(name) = task_names
        .iter()
        .filter(|name| name.as_str() != "kernel")
        .find(|name| !all_tasks.contains(name))
    {
        bail!(toml.task_name_suggestion(name))
    }
    Ok(())
}

/// Checks the buildstamp file and runs `cargo clean` if invalid
fn check_rebuild(toml: &Config) -> Result<()> {
    let buildstamp_file = Path::new("target").join("buildstamp");

    let rebuild = match std::fs::read(&buildstamp_file) {
        Ok(contents) => {
            if let Ok(contents) = std::str::from_utf8(&contents) {
                if let Ok(cmp) = u64::from_str_radix(contents, 16) {
                    toml.buildhash != cmp
                } else {
                    println!("buildstamp file contents unknown; re-building.");
                    true
                }
            } else {
                println!("buildstamp file contents corrupt; re-building.");
                true
            }
        }
        Err(_) => {
            println!("no buildstamp file found; re-building.");
            true
        }
    };
    // if we need to rebuild, we should clean everything before we start building
    if rebuild {
        println!("app.toml has changed; rebuilding all tasks");
        let mut names = vec![toml.kernel.name.as_str()];
        for name in toml.tasks.keys() {
            // This may feel redundant: don't we already have the name?
            // Well, consider our supervisor:
            //
            // [tasks.jefe]
            // name = "task-jefe"
            //
            // The "name" in the key is `jefe`, but the package (crate)
            // name is in `tasks.jefe.name`, and that's what we need to
            // give to `cargo`.
            names.push(toml.tasks[name].name.as_str());
        }
        cargo_clean(&names, &toml.target)?;
    }

    // now that we're clean, update our buildstamp file; any failure to build
    // from here on need not trigger a clean
    std::fs::write(&buildstamp_file, format!("{:x}", toml.buildhash))?;

    Ok(())
}

#[derive(Debug, Hash)]
struct LoadSegment {
    source_file: PathBuf,
    data: Vec<u8>,
}

/// Builds a specific task, return `true` if anything changed
fn build_task(cfg: &PackageConfig, name: &str) -> Result<bool> {
    // Use relocatable linker script for this build
    fs::copy(cfg.arch_consts.rlink_script, "target/link.x")?;
    if cfg.toml.need_tz_linker(&name) {
        fs::copy("build/trustzone.x", "target/trustzone.x")?;
    } else {
        File::create(Path::new("target/trustzone.x"))?;
    }

    let build_config = cfg
        .toml
        .task_build_config(name, cfg.verbose, Some(&cfg.sysroot))
        .unwrap();
    build(cfg, name, build_config, true)
        .context(format!("failed to build {}", name))
}

/// Link a specific task
fn link_task(
    cfg: &PackageConfig,
    name: &str,
    image_name: &str,
    allocs: &Allocations,
) -> Result<()> {
    println!("linking task '{}'", name);
    let task_toml = &cfg.toml.tasks[name];
    generate_task_linker_script(
        cfg.arch_target,
        "memory.x",
        &allocs.tasks[name],
        Some(&task_toml.sections),
        task_toml.stacksize.or(cfg.toml.stacksize).ok_or_else(|| {
            anyhow!("{}: no stack size specified and there is no default", name)
        })?,
        &cfg.toml.all_regions("flash".to_string())?,
        image_name,
    )
    .context(format!("failed to generate linker script for {}", name))?;

    let working_dir = &cfg.dist_dir;
    fs::copy(
        "target/memory.x",
        working_dir.join(format!("{}-memory.x", name)),
    )?;

    fs::copy(&cfg.arch_consts.link_script, "target/link.x")?;
    if cfg.toml.need_tz_linker(&name) {
        fs::copy("build/trustzone.x", "target/trustzone.x")?;
    } else {
        File::create(Path::new("target/trustzone.x"))?;
    }

    // Link the static archive
    link(
        cfg,
        &format!("{}.elf", name),
        &format!("{}/{}", image_name, name),
    )
}

/// Link a specific task using a dummy linker script that
fn link_dummy_task(cfg: &PackageConfig, name: &str) -> Result<()> {
    let task_toml = &cfg.toml.tasks[name];

    let memories: BTreeMap<String, Range<AbiSize>> = cfg
        .toml
        .memories(&cfg.toml.image_names[0])?
        .into_iter()
        .collect();

    generate_task_linker_script(
        cfg.arch_target,
        "memory.x",
        &memories, // ALL THE SPACE
        Some(&task_toml.sections),
        task_toml.stacksize.or(cfg.toml.stacksize).ok_or_else(|| {
            anyhow!("{}: no stack size specified and there is no default", name)
        })?,
        &cfg.toml.all_regions("flash".to_string())?,
        &cfg.toml.image_names[0],
    )
    .context(format!("failed to generate linker script for {}", name))?;

    let working_dir = &cfg.dist_dir;
    fs::copy(
        "target/memory.x",
        working_dir.join(format!("{}-memory.x", name)),
    )?;

    fs::copy(cfg.arch_consts.tlink_script, "target/link.x")?;
    if cfg.toml.need_tz_linker(&name) {
        fs::copy("build/trustzone.x", "target/trustzone.x")?;
    } else {
        File::create(Path::new("target/trustzone.x"))?;
    }

    // Link the static archive
    link(cfg, &format!("{}.elf", name), &format!("{}.tmp", name))
}

fn task_size<'a, 'b>(
    cfg: &'a PackageConfig,
    name: &'b str,
) -> Result<IndexMap<&'a str, u64>> {
    let task = &cfg.toml.tasks[name];
    let stacksize = task.stacksize.or(cfg.toml.stacksize).unwrap();
    load_task_size(&cfg.toml, name, stacksize)
}

/// Loads a given task's ELF file, populating `all_output_sections` and
/// returning its entry point.
fn task_entry_point(
    cfg: &PackageConfig,
    name: &str,
    image_name: &str,
    all_output_sections: &mut BTreeMap<AbiSize, LoadSegment>,
) -> Result<AbiSize> {
    let task_toml = &cfg.toml.tasks[name];
    resolve_task_slots(cfg, name, image_name)?;

    let mut symbol_table = BTreeMap::default();
    let (ep, flash) = load_elf(
        &cfg.img_file(name, image_name),
        all_output_sections,
        &mut symbol_table,
    )?;

    if let Some(required) = task_toml.max_sizes.get("flash") {
        if flash > *required as usize {
            bail!(
                "{} has insufficient flash: specified {} bytes, needs {}",
                task_toml.name,
                required,
                flash
            );
        }
    }
    Ok(ep)
}

fn build_kernel(
    cfg: &PackageConfig,
    allocs: &Allocations,
    all_output_sections: &mut BTreeMap<AbiSize, LoadSegment>,
    all_memories: &IndexMap<String, Range<AbiSize>>,
    entry_points: &HashMap<String, AbiSize>,
    image_name: &str,
    secure: &Option<SecureData>,
) -> Result<(AbiSize, BTreeMap<String, AbiSize>)> {
    let mut image_id = fnv::FnvHasher::default();
    all_output_sections.hash(&mut image_id);

    // Format the descriptors for the kernel build.
    let kconfig = make_kconfig(
        &cfg.toml,
        &allocs.tasks,
        entry_points,
        image_name,
        secure,
    )?;
    let kconfig = ron::ser::to_string(&kconfig)?;

    kconfig.hash(&mut image_id);
    allocs.hash(&mut image_id);

    generate_kernel_linker_script(
        cfg.arch_target,
        "memory.x",
        &allocs.kernel,
        cfg.toml.kernel.stacksize.unwrap_or(DEFAULT_KERNEL_STACK),
        &cfg.toml.image_memories("flash".to_string())?,
    )?;

    fs::copy(&cfg.arch_consts.kernel_link_script, "target/link.x")?;

    let image_id = image_id.finish();

    // Build the kernel.
    let build_config = cfg.toml.kernel_build_config(
        cfg.verbose,
        &[
            ("HUBRIS_KCONFIG", &kconfig),
            ("HUBRIS_IMAGE_ID", &format!("{}", image_id)),
        ],
        Some(&cfg.sysroot),
    );
    build(cfg, "kernel", build_config, false)?;
    if update_image_header(
        &cfg.dist_file("kernel"),
        &cfg.img_file("kernel.modified", image_name),
        all_memories,
        all_output_sections,
        secure,
    )? {
        std::fs::copy(
            &cfg.dist_file("kernel"),
            cfg.img_file("kernel.orig", image_name),
        )?;
        std::fs::copy(
            &cfg.img_file("kernel.modified", image_name),
            cfg.img_file("kernel", image_name),
        )?;
    } else {
        std::fs::copy(
            &cfg.dist_file("kernel"),
            cfg.img_file("kernel", image_name),
        )?;
    }

    let mut ksymbol_table = BTreeMap::default();
    let (kentry, _) = load_elf(
        &cfg.img_file("kernel", image_name),
        all_output_sections,
        &mut ksymbol_table,
    )?;
    Ok((kentry, ksymbol_table))
}

/// Adjusts the hubris image header in the ELF file.
/// Returns true if the header was found and updated,
/// false otherwise.
fn update_image_header(
    input: &Path,
    output: &Path,
    map: &IndexMap<String, Range<AbiSize>>,
    all_output_sections: &mut BTreeMap<AbiSize, LoadSegment>,
    secure: &Option<SecureData>,
) -> Result<bool> {
    let mut file_image = std::fs::read(input)?;
    let elf = goblin::elf::Elf::parse(&file_image)?;

    if elf.header.e_machine != goblin::elf::header::EM_ARM
        && elf.header.e_machine != goblin::elf::header::EM_RISCV
    {
        bail!("this is not an ARM or RISC-V file");
    }

    // Good enough.
    for sec in &elf.section_headers {
        if let Some(name) = elf.shdr_strtab.get_at(sec.sh_name) {
            if name == ".header"
                && (sec.sh_size as usize)
                    >= core::mem::size_of::<abi::ImageHeader>()
            {
                let flash = map.get("flash").unwrap();

                // Compute the total image size by finding the highest address
                // from all the tasks built. Because this is the kernel all
                // tasks must be built
                let mut end = 0;

                for (addr, sec) in all_output_sections {
                    if (*addr as AbiSize) > flash.start
                        && (*addr as AbiSize) < flash.end
                        && (*addr as AbiSize) > end
                    {
                        end = addr + (sec.data.len() as AbiSize);
                    }
                }

                let len = end - flash.start;

                let mut header = abi::ImageHeader {
                    magic: abi::HEADER_MAGIC,
                    total_image_len: len as AbiSize,
                    ..Default::default()
                };

                let last = if let Some(s) = secure {
                    let mut i = 0;

                    // Our memory layout with a secure task looks like the
                    // following:
                    // +---------------+
                    // |               |
                    // |   Task        |
                    // | (Non-secure)  |
                    // |               |
                    // |               |
                    // +---------------+
                    // |               |
                    // |   Task        |
                    // | (Non-secure)  |
                    // |               |
                    // |               |
                    // +---------------+
                    // |               |
                    // |   Task        |
                    // | (Secure)      |
                    // +---------------+
                    // |    NSC        |
                    // +---------------+
                    // |               |
                    // |   Task        |
                    // | (Non-secure)  |
                    // |               |
                    // |               |
                    // +---------------+
                    //
                    // The entries in the SAU specify regions that are
                    // non-secure OR non-secure callable (NSC).
                    // This means the entry for our flash gets broken
                    // down into three entries:
                    // 1) Non-secure range before the secure task
                    // 2) non-secure range after the secure task
                    // 3) NSC region in the secure task
                    for (_, range) in map.iter() {
                        if range.contains(&s.secure.start) {
                            // These values correspond to SAU_RBAR and
                            // SAU_RLAR which are defined in D1.2.221 and
                            // D1.2.222 of the ARMv8m manual
                            //
                            // Bit0 of RLAR indicates a region is valid,
                            // Bit1 indicates that the region is NSC
                            // All entries much be 32-byte aligned
                            header.sau_entries[i].rbar = range.start;
                            header.sau_entries[i].rlar =
                                (s.secure.start - 1) & !0x1f | 1;

                            i += 1;

                            header.sau_entries[i].rbar = s.secure.end;
                            header.sau_entries[i].rlar =
                                (range.end - 1) & !0x1f | 1;

                            i += 1;

                            header.sau_entries[i].rbar = s.nsc.start;
                            header.sau_entries[i].rlar =
                                (s.nsc.end - 1) & !0x1f | 3;

                            i += 1;
                        } else {
                            header.sau_entries[i].rbar = range.start;
                            header.sau_entries[i].rlar =
                                (range.end - 1) & !0x1f | 1;
                            i += 1;
                        }
                    }
                    i
                } else {
                    for (i, (_, range)) in map.iter().enumerate() {
                        header.sau_entries[i].rbar = range.start;
                        header.sau_entries[i].rlar =
                            (range.end - 1) & !0x1f | 1;
                    }

                    map.len()
                };

                // TODO need a better place to put this...
                header.sau_entries[last].rbar = 0x4000_0000;
                header.sau_entries[last].rlar = 0x4fff_ffe0 | 1;

                header
                    .write_to_prefix(
                        &mut file_image[(sec.sh_offset as usize)..],
                    )
                    .unwrap();
                std::fs::write(output, &file_image)?;
                return Ok(true);
            }
        }
    }

    Ok(false)
}

/// Prints warning messages about priority inversions
fn check_task_priorities(toml: &Config) -> Result<()> {
    let idle_priority = toml.tasks["idle"].priority;
    for (i, (name, task)) in toml.tasks.iter().enumerate() {
        for callee in task.task_slots.values() {
            let p = toml
                .tasks
                .get(callee)
                .ok_or_else(|| anyhow!("Invalid task-slot: {}", callee))?
                .priority;
            if p >= task.priority && name != callee {
                // TODO: once all priority inversions are fixed, return an
                // error so no more can be introduced
                eprint!("{}", "Priority inversion: ".red());
                eprintln!(
                    "task {} (priority {}) calls into {} (priority {})",
                    name, task.priority, callee, p
                );
            }
        }
        if task.priority >= idle_priority && name != "idle" {
            bail!("task {} has priority that's >= idle priority", name);
        } else if i == 0 && task.priority != 0 {
            bail!("Supervisor task ({}) is not at priority 0", name);
        } else if i != 0 && task.priority == 0 {
            bail!("Task {} is not the supervisor, but has priority 0", name,);
        }
    }

    Ok(())
}

fn generate_linker_aliases(
    arch_target: ArchTarget,
    linkscr: &mut File,
) -> Result<()> {
    match arch_target {
        ArchTarget::ARM => {}
        ArchTarget::RISCV32 => {
            writeln!(linkscr, "REGION_ALIAS(\"REGION_TEXT\", FLASH);")?;
            writeln!(linkscr, "REGION_ALIAS(\"REGION_RODATA\", FLASH);")?;
            writeln!(linkscr, "REGION_ALIAS(\"REGION_DATA\", RAM);")?;
            writeln!(linkscr, "REGION_ALIAS(\"REGION_BSS\", RAM);")?;
            writeln!(linkscr, "REGION_ALIAS(\"REGION_HEAP\", RAM);")?;
            writeln!(linkscr, "REGION_ALIAS(\"REGION_STACK\", STACK);")?;
        }
        ArchTarget::RISCV64 => {
            writeln!(linkscr, "REGION_ALIAS(\"REGION_TEXT\", RAM);")?;
            writeln!(linkscr, "REGION_ALIAS(\"REGION_RODATA\", RAM);")?;
            writeln!(linkscr, "REGION_ALIAS(\"REGION_DATA\", RAM);")?;
            writeln!(linkscr, "REGION_ALIAS(\"REGION_BSS\", RAM);")?;
            writeln!(linkscr, "REGION_ALIAS(\"REGION_HEAP\", RAM);")?;
            writeln!(linkscr, "REGION_ALIAS(\"REGION_STACK\", STACK);")?;
        }
    }
    Ok(())
}

fn generate_task_linker_script(
    arch_target: ArchTarget,
    name: &str,
    map: &BTreeMap<String, Range<AbiSize>>,
    sections: Option<&IndexMap<String, String>>,
    stacksize: AbiSize,
    images: &IndexMap<String, Range<AbiSize>>,
    image_name: &str,
) -> Result<()> {
    // Put the linker script somewhere the linker can find it
    let mut linkscr = File::create(Path::new(&format!("target/{}", name)))?;

    fn emit(
        linkscr: &mut File,
        sec: &str,
        o: AbiSize,
        l: AbiSize,
    ) -> Result<()> {
        writeln!(
            linkscr,
            "{} (rwx) : ORIGIN = {:#010x}, LENGTH = {:#010x}",
            sec, o, l
        )?;
        Ok(())
    }

    writeln!(linkscr, "MEMORY\n{{")?;
    for (name, range) in map {
        let mut start = range.start;
        let end = range.end;
        let name = name.to_ascii_uppercase();

        // Our stack comes out of RAM
        if name == "RAM" {
            if stacksize & 0x7 != 0 {
                // If we are not 8-byte aligned, the kernel will not be
                // pleased -- and can't be blamed for a little rudeness;
                // check this here and fail explicitly if it's unaligned.
                bail!("specified stack size is not 8-byte aligned");
            }

            emit(&mut linkscr, "STACK", start, stacksize)?;
            start += stacksize;

            if start > end {
                bail!("specified stack size is greater than RAM size");
            }
        }

        emit(&mut linkscr, &name, start, end - start)?;
    }
    writeln!(linkscr, "}}")?;
    for (name, out) in images {
        if name == image_name {
            writeln!(linkscr, "__this_image = {:#010x};", out.start)?;
        }
        writeln!(
            linkscr,
            "__IMAGE_{}_BASE = {:#010x};",
            name.to_ascii_uppercase(),
            out.start
        )?;
        writeln!(
            linkscr,
            "__IMAGE_{}_END = {:#010x};",
            name.to_ascii_uppercase(),
            out.end
        )?;
    }

    // The task may have defined additional section-to-memory mappings.
    if let Some(map) = sections {
        writeln!(linkscr, "SECTIONS {{")?;
        for (section, memory) in map {
            writeln!(linkscr, "  .{} (NOLOAD) : ALIGN(4) {{", section)?;
            writeln!(linkscr, "    *(.{} .{}.*);", section, section)?;
            writeln!(linkscr, "  }} > {}", memory.to_ascii_uppercase())?;
        }
        writeln!(linkscr, "}} INSERT AFTER .uninit")?;
    }

    generate_linker_aliases(arch_target, &mut linkscr)?;

    Ok(())
}

fn generate_kernel_linker_script(
    arch_target: ArchTarget,
    name: &str,
    map: &BTreeMap<String, Range<AbiSize>>,
    stacksize: AbiSize,
    images: &IndexMap<String, Range<AbiSize>>,
) -> Result<()> {
    // Put the linker script somewhere the linker can find it
    let mut linkscr =
        File::create(Path::new(&format!("target/{}", name))).unwrap();

    let mut stack_start = None;
    let mut stack_base = None;

    writeln!(linkscr, "MEMORY\n{{").unwrap();
    for (name, range) in map {
        let mut start = range.start;
        let end = range.end;
        let name = name.to_ascii_uppercase();

        // Our stack comes out of RAM
        if name == "RAM" {
            if stacksize & 0x7 != 0 {
                // If we are not 8-byte aligned, the kernel will not be
                // pleased -- and can't be blamed for a little rudeness;
                // check this here and fail explicitly if it's unaligned.
                bail!("specified kernel stack size is not 8-byte aligned");
            }

            stack_base = Some(start);
            writeln!(
                linkscr,
                "STACK (rw) : ORIGIN = {:#010x}, LENGTH = {:#010x}",
                start, stacksize,
            )?;
            start += stacksize;
            stack_start = Some(start);

            if start > end {
                bail!("specified kernel stack size is greater than RAM size");
            }
        }

        writeln!(
            linkscr,
            "{} (rwx) : ORIGIN = {:#010x}, LENGTH = {:#010x}",
            name,
            start,
            end - start
        )
        .unwrap();
    }

    for (name, out) in images {
        writeln!(
            linkscr,
            "IMAGE{}_FLASH (rwx) : ORIGIN = {:#010x}, LENGTH = {:#010x}",
            name.to_uppercase(),
            out.start,
            out.end - out.start
        )
        .unwrap();
    }
    writeln!(linkscr, "}}").unwrap();
    writeln!(linkscr, "__eheap = ORIGIN(RAM) + LENGTH(RAM);").unwrap();
    writeln!(linkscr, "_stack_base = {:#010x};", stack_base.unwrap()).unwrap();
    writeln!(linkscr, "_stack_start = {:#010x};", stack_start.unwrap())
        .unwrap();

    for (name, _) in images {
        writeln!(
            linkscr,
            "IMAGE{} = ORIGIN(IMAGE{}_FLASH);",
            name.to_uppercase(),
            name.to_uppercase(),
        )
        .unwrap();
    }

    generate_linker_aliases(arch_target, &mut linkscr)?;

    Ok(())
}

fn build(
    cfg: &PackageConfig,
    name: &str,
    build_config: BuildConfig,
    reloc: bool,
) -> Result<bool> {
    println!("building crate {}", build_config.crate_name);

    let mut cmd = build_config.cmd("rustc");
    cmd.arg("--release");

    // We're capturing stderr (for diagnosis), so `cargo` won't automatically
    // turn on color.  If *we* are a TTY, then force it on.
    if atty::is(Stream::Stderr) {
        cmd.arg("--color");
        cmd.arg("always");
    }

    // This works because we control the environment in which we're about
    // to invoke cargo, and never modify CARGO_TARGET in that environment.
    let cargo_out = Path::new("target").to_path_buf();

    let remap_path_prefix: String = cfg
        .remap_paths
        .iter()
        .map(|r| format!(" --remap-path-prefix={}={}", r.0.display(), r.1))
        .collect();

    let mut rustflags: String = format!(
        "
         -C link-arg=-z -C link-arg=common-page-size=0x20 \
         -C link-arg=-z -C link-arg=max-page-size=0x20 \
         -C llvm-args=--enable-machine-outliner=never \
         -C overflow-checks=y \
         -C metadata={} \
         {}
         ",
        cfg.link_script_hash, remap_path_prefix,
    );

    match cfg.arch_target {
        ArchTarget::RISCV32 => {
            rustflags.push_str(" -C link-arg=--orphan-handling=error")
        }
        _ => {}
    }

    cmd.env("RUSTFLAGS", &rustflags);
    cmd.arg("--");
    cmd.arg("-C")
        .arg("link-arg=-Tlink.x")
        .arg("-L")
        .arg(format!("{}", cargo_out.display()));
    if reloc {
        cmd.arg("-C").arg("link-arg=-r");
    }

    if cfg.edges {
        let mut tree = build_config.cmd("tree");
        tree.arg("--edges").arg("features").arg("--verbose");
        println!(
            "Crate: {}\nRunning cargo {:?}",
            build_config.crate_name, tree
        );
        let tree_status = tree
            .status()
            .context(format!("failed to run edge ({:?})", tree))?;
        if !tree_status.success() {
            bail!("tree command failed, see output for details");
        }
    }

    // File generated by the build system
    let src_file = cargo_out.join(build_config.out_path);
    let prev_time = std::fs::metadata(&src_file).and_then(|f| f.modified());

    let status = cmd
        .output()
        .context(format!("failed to run rustc ({:?})", cmd))?;

    // Immediately echo `stderr` back out, using a raw write because it may
    // contain terminal control characters
    std::io::stderr().write_all(&status.stderr)?;

    if !status.status.success() {
        // We've got a special case here: if the kernel memory is too small,
        // then the build will fail with a cryptic linker error.  We can't
        // convert `status.stderr` to a `String`, because it probably contains
        // terminal control characters, so do a raw `&[u8]` search instead.
        if name == "kernel"
            && memchr::memmem::find(
                &status.stderr,
                b"will not fit in region".as_slice(),
            )
            .is_some()
        {
            bail!(
                "command failed, see output for details\n    \
                         The kernel may have run out of space; try increasing \
                         its allocation in the app's TOML file"
            )
        }
        bail!("command failed, see output for details");
    }

    // Destination where it should be copied (using the task name rather than
    // the crate name)
    let dest = cfg.dist_file(if reloc {
        format!("{}.elf", name)
    } else {
        name.to_string()
    });

    // Compare file times to see if it has been modified
    let newer = match prev_time {
        Err(_) => true,
        Ok(prev_time) => std::fs::metadata(&src_file)?.modified()? > prev_time,
    };
    let changed = newer || !dest.exists();

    if changed {
        println!("{} -> {}", src_file.display(), dest.display());
        std::fs::copy(&src_file, dest)?;
    } else {
        println!("{} (unchanged)", dest.display());
    }

    Ok(changed)
}

fn link(
    cfg: &PackageConfig,
    src_file: impl AsRef<Path> + AsRef<std::ffi::OsStr>,
    dst_file: impl AsRef<Path> + AsRef<std::ffi::OsStr>,
) -> Result<()> {
    let mut ld = cfg.sysroot.clone();
    for p in ["lib", "rustlib", &cfg.host_triple, "bin", "gcc-ld", "ld"] {
        ld.push(p);
    }
    let mut cmd = Command::new(ld);
    if cfg.verbose {
        cmd.arg("--verbose");
    }

    // We expect the caller to set up our linker scripts, but copy them into
    // our working directory here
    let working_dir = &cfg.dist_dir;
    for f in ["link.x", "memory.x", "trustzone.x"] {
        std::fs::copy(format!("target/{}", f), working_dir.join(f))
            .context(format!("Could not copy {} to link dir", f))?;
    }
    assert!(AsRef::<Path>::as_ref(&src_file).is_relative());
    assert!(AsRef::<Path>::as_ref(&dst_file).is_relative());

    let m = match cfg.toml.target.as_str() {
        "thumbv6m-none-eabi"
        | "thumbv7em-none-eabihf"
        | "thumbv8m.main-none-eabihf" => "armelf",
        "riscv32imc-unknown-none-elf" | "riscv32imac-unknown-none-elf" => {
            "elf32lriscv"
        }
        "riscv64imac-unknown-none-elf" => "elf64lriscv",
        _ => bail!("No target emulation for '{}'", cfg.toml.target),
    };
    cmd.arg(src_file);
    cmd.arg("-o").arg(dst_file);
    cmd.arg("-Tlink.x");
    cmd.arg("--gc-sections");
    cmd.arg("-m").arg(m);
    cmd.arg("-z").arg("common-page-size=0x20");
    cmd.arg("-z").arg("max-page-size=0x20");
    cmd.arg("-rustc-lld-flavor=ld");

    cmd.current_dir(working_dir);

    let status = cmd
        .status()
        .context(format!("failed to run linker ({:?})", cmd))?;

    if !status.success() {
        bail!("command failed, see output for details");
    }

    Ok(())
}

#[derive(Debug, Clone, Default, Hash)]
pub struct Allocations {
    /// Map from memory-name to address-range
    pub kernel: BTreeMap<String, Range<AbiSize>>,
    /// Map from task-name to memory-name to address-range
    pub tasks: BTreeMap<String, BTreeMap<String, Range<AbiSize>>>,
}

/// Allocates address space from all regions for the kernel and all tasks.
///
/// The allocation strategy is slightly involved, because of the limitations of
/// the ARMv7-M MPU. (Currently we use the same strategy on ARMv8-M even though
/// it's more flexible.)
///
/// Address space regions are required to be power-of-two in size and naturally
/// aligned. In other words, all the addresses in a single region must have some
/// number of top bits the same, and any combination of bottom bits.
///
/// To complicate things,
///
/// - There's no particular reason why the memory regions defined in the
///   app.toml need to be aligned to any particular power of two.
/// - When there's a bootloader added to the image, it will bump a nicely
///   aligned starting address forward by a few kiB anyway.
/// - Said bootloader requires the kernel text to appear immediately after it in
///   ROM, so, the kernel must be laid down first. (This is not true of RAM, but
///   putting the kernel first in RAM has some useful benefits.)
///
/// The method we're using here is essentially the "deallocate" side of a
/// power-of-two buddy allocator, only simplified because we're using it to
/// allocate a series of known sizes.
///
/// To allocate space for a single request, we
///
/// - Check the alignment of the current position pointer.
/// - Find the largest pending request of that alignment _or less._
/// - If we found one, bump the pointer forward and repeat.
/// - If not, find the smallest pending request that requires greater alignment,
///   and skip address space until it can be satisfied, and then repeat.
///
/// This means that the algorithm needs to keep track of a queue of pending
/// requests per alignment size.
pub fn allocate_all(
    toml: &Config,
    task_sizes: &HashMap<&str, IndexMap<&str, u64>>,
) -> Result<BTreeMap<String, AllocationMap>> {
    // Collect all allocation requests into queues, one per memory type, indexed
    // by allocation size. This is equivalent to required alignment because of
    // the naturally-aligned-power-of-two requirement.
    //
    // We keep kernel and task requests separate so we can always service the
    // kernel first.
    //
    // The task map is: memory name -> allocation size -> queue of task name.
    // The kernel map is: memory name -> allocation size
    let kernel = &toml.kernel;
    let tasks = &toml.tasks;
    let mut result: BTreeMap<
        String,
        (Allocations, IndexMap<String, Range<AbiSize>>),
    > = BTreeMap::new();

    for image_name in &toml.image_names {
        let mut allocs = Allocations::default();
        let mut free = toml.memories(image_name)?;
        let kernel_requests = &kernel.requires;

        let mut task_requests: BTreeMap<
            &str,
            BTreeMap<AbiSize, VecDeque<&str>>,
        > = BTreeMap::new();

        for name in tasks.keys() {
            for (mem, amt) in task_sizes[name.as_str()].iter() {
                let bytes = toml.suggest_memory_region_size(name, *amt);
                if let Some(r) = tasks[name].max_sizes.get(&mem.to_string()) {
                    if bytes > *r as u64 {
                        bail!(
                        "task {}: needs {} bytes of {} but max-sizes limits it to {}",
                        name, bytes, mem, r);
                    }
                }
                task_requests
                    .entry(mem)
                    .or_default()
                    .entry(bytes.try_into().unwrap())
                    .or_default()
                    .push_back(name.as_str());
            }
        }

        // Okay! Do memory types one by one, fitting kernel first.
        for (region, avail) in &mut free {
            let mut k_req = kernel_requests.get(region.as_str());
            let mut t_reqs = task_requests.get_mut(region.as_str());

            fn reqs_map_not_empty(
                om: &Option<&mut BTreeMap<AbiSize, VecDeque<&str>>>,
            ) -> bool {
                om.iter()
                    .flat_map(|map| map.values())
                    .any(|q| !q.is_empty())
            }

            'fitloop: while k_req.is_some() || reqs_map_not_empty(&t_reqs) {
                let align = if avail.start == 0 {
                    // Lie to keep the masks in range. This could be avoided by
                    // tracking log2 of masks rather than masks.
                    1 << 31
                } else {
                    1 << avail.start.trailing_zeros()
                };

                // Search order is:
                // - Kernel.
                // - Task requests equal to or smaller than this alignment, in
                //   descending order of size.
                // - Task requests larger than this alignment, in ascending order of
                //   size.

                if let Some(&sz) = k_req.take() {
                    // The kernel wants in on this.
                    allocs.kernel.insert(
                        region.to_string(),
                        allocate_k(region, sz, avail)?,
                    );
                    continue 'fitloop;
                }

                if let Some(t_reqs) = t_reqs.as_mut() {
                    for (&sz, q) in t_reqs.range_mut(..=align).rev() {
                        if let Some(task) = q.pop_front() {
                            // We can pack an equal or smaller one in.
                            let align = toml.task_memory_alignment(sz);
                            allocs
                                .tasks
                                .entry(task.to_string())
                                .or_default()
                                .insert(
                                    region.to_string(),
                                    allocate_one(region, sz, align, avail)?,
                                );
                            continue 'fitloop;
                        }
                    }

                    for (&sz, q) in t_reqs.range_mut(align + 1..) {
                        if let Some(task) = q.pop_front() {
                            // We've gotta use a larger one.
                            let align = toml.task_memory_alignment(sz);
                            allocs
                                .tasks
                                .entry(task.to_string())
                                .or_default()
                                .insert(
                                    region.to_string(),
                                    allocate_one(region, sz, align, avail)?,
                                );
                            continue 'fitloop;
                        }
                    }
                }

                // If we reach this point, it means our loop condition is wrong,
                // because one of the above things should really have happened.
                // Panic here because otherwise it's a hang.
                panic!("loop iteration without progess made!");
            }
        }

        result.insert(image_name.to_string(), (allocs, free));
    }
    Ok(result)
}

fn allocate_k(
    region: &str,
    size: AbiSize,
    avail: &mut Range<AbiSize>,
) -> Result<Range<AbiSize>> {
    // Our base address will be larger than avail.start if it doesn't meet our
    // minimum requirements. Round up.
    let base = (avail.start + 15) & !15;

    if !avail.contains(&(base + size - 1)) {
        bail!(
            "out of {}: can't allocate {} more after base {:x}",
            region,
            size,
            base
        )
    }

    let end = base + size;
    // Update the available range to exclude what we've taken.
    avail.start = end;

    Ok(base..end)
}

fn allocate_one(
    region: &str,
    size: AbiSize,
    align: AbiSize,
    avail: &mut Range<AbiSize>,
) -> Result<Range<AbiSize>> {
    assert!(align.is_power_of_two());

    let size_mask = align - 1;

    // Our base address will be larger than avail.start if it doesn't meet our
    // minimum requirements. Round up.
    let base = (avail.start + size_mask) & !size_mask;

    if base >= avail.end || size > avail.end - base {
        bail!(
            "out of {}: can't allocate {} more after base {:x}",
            region,
            size,
            base
        )
    }

    let end = base + size;
    // Update the available range to exclude what we've taken.
    avail.start = end;

    Ok(base..end)
}

#[derive(Serialize)]
pub struct KernelConfig {
    tasks: Vec<abi::TaskDesc>,
    regions: Vec<abi::RegionDesc>,
    irqs: Vec<abi::Interrupt>,
    timer: (AbiSize, AbiSize),
}

/// Generate the application descriptor table that the kernel uses to find and
/// start tasks.
///
/// The layout of the table is a series of structs from the `abi` crate:
///
/// - One `App` header.
/// - Some number of `RegionDesc` records describing memory regions.
/// - Some number of `TaskDesc` records describing tasks.
/// - Some number of `Interrupt` records routing interrupts to tasks.
pub fn make_kconfig(
    toml: &Config,
    task_allocations: &BTreeMap<String, BTreeMap<String, Range<AbiSize>>>,
    entry_points: &HashMap<String, AbiSize>,
    image_name: &str,
    secure: &Option<SecureData>,
) -> Result<KernelConfig> {
    // Generate the three record sections concurrently.
    let mut regions = vec![];
    let mut task_descs = vec![];
    let mut irqs = vec![];
    let mut timer = (0x0, 0x0);

    // Region 0 is the NULL region, used as a placeholder. It gives no access to
    // memory.
    regions.push(abi::RegionDesc {
        base: 0,
        size: 32, // smallest legal size on ARMv7-M
        attributes: abi::RegionAttributes::empty(), // no rights
    });

    // Regions 1.. are the fixed peripheral regions, shared by tasks that
    // reference them. We'll build a lookup table so we can find them
    // efficiently by name later.
    let mut peripheral_index = IndexMap::new();
    let sname = &"secure".to_string();

    // Build a set of all peripheral names used by tasks, which we'll use
    // to filter out unused peripherals.
    let used_peripherals = toml
        .tasks
        .iter()
        .flat_map(|(_name, task)| task.uses.iter())
        .collect::<HashSet<_>>();

    let power_of_two_required = toml.mpu_power_of_two_required();

    for (name, p) in toml.peripherals.iter() {
        // TODO: Get rid of this eventually and make a proper implementation of
        //       the configuration for these peripherals.
        if toml.target.as_str().contains("riscv32") {
            if name == "mtime" {
                timer.0 = p.address;
                continue;
            } else if name == "mtimecmp" {
                timer.1 = p.address;
                continue;
            }
        }
        if power_of_two_required && !p.size.is_power_of_two() {
            panic!("Memory region for peripheral '{}' is required to be a power of two, but has size {}", name, p.size);
        }

        // skip periperhals that aren't in at least one task's `uses`
        if !used_peripherals.contains(name) {
            continue;
        }

        peripheral_index.insert(name, regions.len());

        // Peripherals are always mapped as Device + Read + Write.
        let attributes = abi::RegionAttributes::DEVICE
            | abi::RegionAttributes::READ
            | abi::RegionAttributes::WRITE;

        regions.push(abi::RegionDesc {
            base: p.address,
            size: p.size,
            attributes,
        });
    }

    for (name, p) in toml.extratext.iter() {
        if power_of_two_required && !p.size.is_power_of_two() {
            panic!("Memory region for extratext '{}' is required to be a power of two, but has size {}", name, p.size);
        }

        peripheral_index.insert(name, regions.len());

        // Extra text is marked as read/execute
        let attributes =
            abi::RegionAttributes::READ | abi::RegionAttributes::EXECUTE;

        regions.push(abi::RegionDesc {
            base: p.address,
            size: p.size,
            attributes,
        });
    }

    if let Some(s) = secure {
        peripheral_index.insert(sname, regions.len());

        // Entry point needs to be read/execute
        let attributes =
            abi::RegionAttributes::READ | abi::RegionAttributes::EXECUTE;
        regions.push(abi::RegionDesc {
            base: s.nsc.start,
            size: s.nsc.end - s.nsc.start,
            attributes,
        });
    }

    // The remaining regions are allocated to tasks on a first-come first-serve
    // basis. We don't check power-of-two requirements in task_allocations
    // because it's the result of autosizing, which already takes the MPU into
    // account.
    for (i, (name, task)) in toml.tasks.iter().enumerate() {
        // Regions are referenced by index into the table we just generated.
        // Each task has up to 8, chosen from its 'requires' and 'uses' keys.
        let mut task_regions = [0; 8];

        if task.uses.len() + task_allocations[name].len() > 8 {
            panic!(
                "task {} uses {} peripherals and {} memories (too many)",
                name,
                task.uses.len(),
                task_allocations[name].len()
            );
        }

        // Generate a RegionDesc for each uniquely allocated memory region
        // referenced by this task, and install them as entries 0..N in the
        // task's region table.
        let allocs = &task_allocations[name];
        for (ri, (output_name, range)) in allocs.iter().enumerate() {
            let region: Vec<&crate::config::Output> = toml.outputs[output_name]
                .iter()
                .filter(|o| o.name == *image_name)
                .collect();

            if region.len() > 1 {
                bail!("Multiple regions defined for image {}", image_name);
            }

            let out = region[0];

            let mut attributes = abi::RegionAttributes::empty();
            if out.read {
                attributes |= abi::RegionAttributes::READ;
            }
            if out.write {
                attributes |= abi::RegionAttributes::WRITE;
            }
            if out.execute {
                attributes |= abi::RegionAttributes::EXECUTE;
            }
            if out.dma {
                attributes |= abi::RegionAttributes::DMA;
            }
            // no option for setting DEVICE for this region

            task_regions[ri] = regions.len() as u8;

            regions.push(abi::RegionDesc {
                base: range.start,
                size: range.end - range.start,
                attributes,
            });
        }

        // For peripherals referenced by the task, we don't need to allocate
        // _new_ regions, since we did them all in advance. Just record the
        // entries for the TaskDesc.
        for (j, peripheral_name) in task.uses.iter().enumerate() {
            if let Some(&peripheral) = peripheral_index.get(&peripheral_name) {
                task_regions[allocs.len() + j] = peripheral as u8;
            } else {
                bail!(
                    "Could not find peripheral `{}` referenced by task `{}`.",
                    peripheral_name,
                    name
                );
            }
        }

        let mut flags = abi::TaskFlags::empty();
        if task.start {
            flags |= abi::TaskFlags::START_AT_BOOT;
        }

        task_descs.push(abi::TaskDesc {
            regions: task_regions,
            entry_point: entry_points[name],
            initial_stack: task_allocations[name]["ram"].start
                + task.stacksize.or(toml.stacksize).unwrap(),
            priority: task.priority,
            flags,
            index: u16::try_from(i).expect("more than 2**16 tasks?"),
        });

        // Interrupts.
        for (irq_str, &notification) in &task.interrupts {
            // The irq_str can be either a base-ten number, or a reference to a
            // peripheral. Distinguish them based on whether it parses as an
            // integer.
            match irq_str.parse::<u32>() {
                Ok(irq_num) => {
                    // While it's possible to conceive of a world in which one
                    // might want to have a single interrupt set multiple
                    // notification bits, it's much easier to conceive of a
                    // world in which one has misunderstood that the second
                    // number in the interrupt tuple is in fact a mask, not an
                    // index.
                    if notification.count_ones() != 1 {
                        bail!(
                            "task {}: IRQ {}: notification mask (0b{:b}) \
                             has {} bits set (expected exactly one)",
                            name,
                            irq_str,
                            notification,
                            notification.count_ones()
                        );
                    }

                    irqs.push(abi::Interrupt {
                        irq: abi::InterruptNum(irq_num),
                        owner: abi::InterruptOwner {
                            task: i as u32,
                            notification,
                        },
                    });
                }
                Err(_) => {
                    // This might be an error, or might be a peripheral
                    // reference.
                    //
                    // Peripheral references are of the form "P.I", where P is
                    // the peripheral name and I is the name of one of the
                    // peripheral's defined interrupts.
                    if let Some(dot_pos) =
                        irq_str.bytes().position(|b| b == b'.')
                    {
                        let (pname, iname) = irq_str.split_at(dot_pos);
                        let iname = &iname[1..];
                        let periph =
                            toml.peripherals.get(pname).ok_or_else(|| {
                                anyhow!(
                                    "task {} IRQ {} references peripheral {}, \
                                 which does not exist.",
                                    name,
                                    irq_str,
                                    pname,
                                )
                            })?;
                        let irq_num =
                            periph.interrupts.get(iname).ok_or_else(|| {
                                anyhow!(
                                    "task {} IRQ {} references interrupt {} \
                                 on peripheral {}, but that interrupt name \
                                 is not defined for that peripheral.",
                                    name,
                                    irq_str,
                                    iname,
                                    pname,
                                )
                            })?;
                        irqs.push(abi::Interrupt {
                            irq: abi::InterruptNum(*irq_num),
                            owner: abi::InterruptOwner {
                                task: i as u32,
                                notification,
                            },
                        });
                    } else {
                        bail!(
                            "task {}: IRQ name {} does not match any \
                             known peripheral interrupt, and is not an \
                             integer.",
                            name,
                            irq_str,
                        );
                    }
                }
            }
        }
    }

    if toml.target.as_str().contains("riscv32")
        && ((timer.0 == 0x0) || (timer.1 == 0x0))
    {
        bail!("mtime or mtimecmp has not been set.");
    }

    Ok(KernelConfig {
        irqs,
        tasks: task_descs,
        regions,
        timer,
    })
}

/// Loads an SREC file into the same representation we use for ELF. This is
/// currently unused, but I'm keeping it compiling as proof that it's possible,
/// because we may need it later.
#[allow(dead_code)]
fn load_srec(
    input: &Path,
    output: &mut BTreeMap<u32, LoadSegment>,
) -> Result<u32> {
    let srec_text = std::fs::read_to_string(input)?;
    for record in srec::reader::read_records(&srec_text) {
        let record = record?;
        match record {
            srec::Record::S3(data) => {
                // Check for address overlap
                let range =
                    data.address.0..data.address.0 + data.data.len() as u32;
                if let Some(overlap) = output.range(range.clone()).next() {
                    bail!(
                        "{}: record address range {:x?} overlaps {:x}",
                        input.display(),
                        range,
                        overlap.0
                    )
                }
                output.insert(
                    data.address.0 as u32,
                    LoadSegment {
                        source_file: input.into(),
                        data: data.data,
                    },
                );
            }
            srec::Record::S7(srec::Address32(e)) => return Ok(e),
            _ => (),
        }
    }

    panic!("SREC file missing terminating S7 record");
}

fn load_elf(
    input: &Path,
    output: &mut BTreeMap<AbiSize, LoadSegment>,
    symbol_table: &mut BTreeMap<String, AbiSize>,
) -> Result<(AbiSize, usize)> {
    use goblin::elf::program_header::PT_LOAD;

    let file_image = std::fs::read(input)?;
    let elf = goblin::elf::Elf::parse(&file_image)?;

    if elf.header.e_machine != goblin::elf::header::EM_ARM
        && elf.header.e_machine != goblin::elf::header::EM_RISCV
    {
        bail!("this is not an ARM or RISC-V file");
    }

    let mut flash = 0;

    // Good enough.
    for phdr in &elf.program_headers {
        // Skip sections that aren't intended to be loaded.
        if phdr.p_type != PT_LOAD {
            continue;
        }
        let offset = phdr.p_offset as usize;
        let size = phdr.p_filesz as usize;
        // Note that we are using Physical, i.e. LOADADDR, rather than virtual.
        // This distinction is important for things like the rodata image, which
        // is loaded in flash but expected to be copied to RAM.
        let addr = phdr.p_paddr as AbiSize;

        flash += size;

        // We use this function to re-load an ELF file after we've modfified
        // it. Don't check for overlap if this happens.
        if !output.contains_key(&addr) {
            let range = addr..addr + size as AbiSize;
            if let Some(overlap) = output.range(range.clone()).next() {
                if overlap.1.source_file != input {
                    bail!(
                        "{}: address range {:x?} overlaps {:x} \
                    (from {}); does {} have an insufficient amount of flash?",
                        input.display(),
                        range,
                        overlap.0,
                        overlap.1.source_file.display(),
                        input.display(),
                    );
                } else {
                    bail!(
                        "{}: ELF file internally inconsistent: \
                    address range {:x?} overlaps {:x}",
                        input.display(),
                        range,
                        overlap.0,
                    );
                }
            }
        }

        output.insert(
            addr,
            LoadSegment {
                source_file: input.into(),
                data: file_image[offset..offset + size].to_vec(),
            },
        );
    }

    for s in elf.syms.iter() {
        let index = s.st_name;

        if let Some(name) = elf.strtab.get_at(index) {
            symbol_table.insert(name.to_string(), s.st_value as AbiSize);
        }
    }

    // Return both our entry and the total allocated flash, allowing the
    // caller to assure that the allocated flash does not exceed the task's
    // required flash
    Ok((elf.header.e_entry as AbiSize, flash))
}

/// Keeps track of a build archive being constructed.
struct Archive {
    /// Place where we'll put the final zip file.
    final_path: PathBuf,
    /// Name of temporary file used during construction.
    tmp_path: PathBuf,
    /// ZIP output to the temporary file.
    inner: zip::ZipWriter<File>,
    /// Options used for every file.
    opts: zip::write::FileOptions,
}

impl Archive {
    /// Creates a new build archive that will, when finished, be placed at
    /// `dest`.
    fn new(dest: impl AsRef<Path>) -> Result<Self> {
        let final_path = PathBuf::from(dest.as_ref());

        let mut tmp_path = final_path.clone();
        tmp_path.set_extension("zip.partial");

        let archive = File::create(&tmp_path)?;
        let mut inner = zip::ZipWriter::new(archive);
        inner.set_comment("hubris build archive v3");
        Ok(Self {
            final_path,
            tmp_path,
            inner,
            opts: zip::write::FileOptions::default()
                .compression_method(zip::CompressionMethod::Bzip2),
        })
    }

    /// Copies the file at `src_path` into the build archive at `zip_path`.
    fn copy(
        &mut self,
        src_path: impl AsRef<Path>,
        zip_path: impl AsRef<Path>,
    ) -> Result<()> {
        let mut input = File::open(src_path)?;
        self.inner
            .start_file_from_path(zip_path.as_ref(), self.opts)?;
        std::io::copy(&mut input, &mut self.inner)?;
        Ok(())
    }

    /// Creates a text file in the archive at `zip_path` with `contents`.
    fn text(
        &mut self,
        zip_path: impl AsRef<Path>,
        contents: impl AsRef<str>,
    ) -> Result<()> {
        self.inner
            .start_file_from_path(zip_path.as_ref(), self.opts)?;
        self.inner.write_all(contents.as_ref().as_bytes())?;
        Ok(())
    }

    /// Completes the archive and moves it to its intended location.
    ///
    /// If you drop an `Archive` without calling this, it will leave a temporary
    /// file rather than creating the final archive.
    fn finish(self) -> Result<()> {
        let Self {
            tmp_path,
            final_path,
            mut inner,
            ..
        } = self;
        inner.finish()?;
        drop(inner);
        std::fs::rename(tmp_path, final_path)?;
        Ok(())
    }
}

/// Gets the status of a git repository containing the current working
/// directory. Returns two values:
///
/// - A `String` containing the git commit hash.
/// - A `bool` indicating whether the repository has uncommitted changes.
fn get_git_status() -> Result<(String, bool)> {
    let mut cmd = Command::new("git");
    cmd.arg("rev-parse").arg("HEAD");
    let out = cmd.output()?;
    if !out.status.success() {
        bail!("git rev-parse failed");
    }
    let rev = std::str::from_utf8(&out.stdout)?.trim().to_string();

    let mut cmd = Command::new("git");
    cmd.arg("diff-index").arg("--quiet").arg("HEAD").arg("--");
    let status = cmd
        .status()
        .context(format!("failed to get git status ({:?})", cmd))?;

    Ok((rev, !status.success()))
}

fn binary_to_srec(
    binary: &Path,
    bin_addr: AbiSize,
    entry: AbiSize,
    out: &Path,
) -> Result<()> {
    let mut srec_out = vec![srec::Record::S0("signed".to_string())];

    let binary = std::fs::read(binary)?;

    let mut addr = bin_addr.try_into()?;
    for chunk in binary.chunks(255 - 5) {
        srec_out.push(srec::Record::S3(srec::Data {
            address: srec::Address32(addr),
            data: chunk.to_vec(),
        }));
        addr += chunk.len() as u32;
    }

    let out_sec_count = srec_out.len() - 1; // header
    if out_sec_count < 0x1_00_00 {
        srec_out.push(srec::Record::S5(srec::Count16(out_sec_count as u16)));
    } else if out_sec_count < 0x1_00_00_00 {
        srec_out.push(srec::Record::S6(srec::Count24(out_sec_count as u32)));
    } else {
        panic!("SREC limit of 2^24 output sections exceeded");
    }

    srec_out.push(srec::Record::S7(srec::Address32(entry.try_into()?)));

    let srec_image = srec::writer::generate_srec_file(&srec_out);
    std::fs::write(out, srec_image)?;
    Ok(())
}

macro_rules! make_header_containers {
    ($abisize:literal,
     $program_headers:ident,
     $section_headers:ident) => {
        paste! {
            let mut $program_headers: Vec<
                goblin::[<elf $abisize>]::program_header::ProgramHeader,
            > = Vec::new();
            let mut $section_headers: Vec<
                goblin::[<elf $abisize>]::section_header::SectionHeader,
            > = Vec::new();
        }
    };
}

macro_rules! make_program_header_common {
    ($program_header:ty, $abisize:ty, $file_offset:expr, $mem_address:expr, $program_size:expr, $alignment:literal, $collection:ident) => {
        paste! {
            use $program_header as [<ph_ $abisize>];
            use [<ph_ $abisize>]::{PF_R, PF_W, PF_X, PT_LOAD};
            $collection.push([<ph_ $abisize>]::ProgramHeader {
                p_type: PT_LOAD,
                p_flags: PF_X | PF_W | PF_R,
                p_offset: $file_offset as $abisize,
                p_vaddr: $mem_address as $abisize,
                p_paddr: $mem_address as $abisize,
                p_filesz: $program_size as $abisize,
                p_memsz: $program_size as $abisize,
                p_align: $alignment, // This matches the alignment guarantees of the kernel & task build
            });
        }
    };
}

macro_rules! make_program_header {
    ($abisize:literal,
     $file_offset:expr,
     $mem_address:expr,
     $program_size:expr,
     $alignment:literal,
     $collection:ident) => {
        paste! {
            make_program_header_common!(
                goblin::[<elf $abisize>]::program_header,
                [<u $abisize>],
                $file_offset,
                $mem_address,
                $program_size,
                $alignment,
                $collection
            );
        }
    };
}

macro_rules! make_section_header_common {
    ($section_header:ty, $abisize:ty, $section_type:expr, $section_flags:expr, $name_offset:expr, $file_offset:expr, $program_size:expr, $mem_address:expr, $alignment:literal, $collection:ident) => {
        paste! {
            use $section_header as [<sh_ $abisize>];
            $collection.push([<sh_ $abisize>]::SectionHeader {
                sh_type: $section_type,
                sh_flags: $section_flags as $abisize,
                sh_name: $name_offset as u32,
                sh_offset: $file_offset as $abisize,
                sh_size: $program_size as $abisize,
                sh_addr: $mem_address as $abisize,
                sh_addralign: $alignment,
                sh_entsize: 0, // No fixed-size entries here
                sh_link: 0,
                sh_info: 0,
            });
        }
    };
}

macro_rules! make_section_header {
    ($abisize:literal,
     $section_type:expr,
     $section_flags:expr,
     $name_offset:expr,
     $file_offset:expr,
     $program_size:expr,
     $mem_address:expr,
     $alignment:literal,
     $collection:ident) => {
        paste! {
            make_section_header_common!(
                goblin::elf::section_header::[<section_header $abisize>],
                [<u $abisize>],
                $section_type,
                $section_flags,
                $name_offset,
                $file_offset,
                $program_size,
                $mem_address,
                $alignment,
                $collection
            );
        }
    };
}

macro_rules! make_header_common {
    ($var:ident, $header:ty, $elfclass:expr, $le_be:expr, $abisize:ty, $entry:expr, $section_offset:expr, $program_headers:ident, $section_headers:ident, $section_name_offset:expr, $ctx:ident) => {
        paste! {
            use $header as [<h_ $abisize>];
            let mut $var = [<h_ $abisize>]::Header {
                e_ident: [
                    127,
                    69,
                    76,
                    70,
                    $elfclass,
                    $le_be,
                    [<h_ $abisize>]::EV_CURRENT,
                    [<h_ $abisize>]::ELFOSABI_NONE,
                    0,
                    0,
                    0,
                    0,
                    0,
                    0,
                    0,
                    0,
                ],
                e_type: [<h_ $abisize>]::ET_EXEC,
                e_machine: 0, // Overridden later
                e_version: 1,
                e_entry: $entry as $abisize,
                e_phoff: goblin::elf::Header::size($ctx) as $abisize,
                e_shoff: $section_offset as $abisize,
                e_flags: 0,
                e_ehsize: goblin::elf::Header::size($ctx) as u16,
                e_phentsize: goblin::elf::ProgramHeader::size($ctx) as u16,
                e_phnum: $program_headers.len() as u16,
                e_shentsize: goblin::elf::SectionHeader::size($ctx) as u16,
                e_shnum: $section_headers.len() as u16,
                e_shstrndx: $section_name_offset as u16,
            };
        };
    };
}

macro_rules! make_header {
    ($abisize:literal,
     le,
     $var:ident,
     $entry:expr,
     $section_offset:expr,
     $program_headers:ident,
     $section_headers:ident,
     $section_name_offset:expr,
     $ctx:ident) => {
        paste! {
            make_header_common! {
                $var,
                goblin::[<elf $abisize>]::header,
                goblin::[<elf $abisize>]::header::[<ELFCLASS $abisize>],
                goblin::[<elf $abisize>]::header::ELFDATA2LSB,
                [<u $abisize>],
                $entry,
                $section_offset,
                $program_headers,
                $section_headers,
                $section_name_offset,
                $ctx
            };
        }
    };
}

fn write_elf(
    sections: &BTreeMap<AbiSize, LoadSegment>,
    kentry: AbiSize,
    cfg: &PackageConfig,
    out: &Path,
) -> Result<()> {
    use goblin::container::{Container, Ctx, Endian};
    use scroll::Pwrite;

    // 'Big' Containers are Goblin for ELF64. 'Little' are ELF32.
    let ctx = Ctx::new(
        if cfg.arch_consts.objcopy_target.starts_with("elf64") {
            Container::Big
        } else {
            Container::Little
        },
        Endian::Little,
    );

    let mut sections_base_address: AbiSize = kentry;
    let mut sections_length: u64 = 0;

    for candidate_section in sections {
        if candidate_section.1.data.len() > 0 {
            if *candidate_section.0 < sections_base_address {
                sections_base_address = *candidate_section.0;
            }

            let end =
                (*candidate_section.0) + candidate_section.1.data.len() as u64;

            if end > sections_length {
                sections_length = end;
            }
        }
    }
    sections_length -= sections_base_address;

    // Create a Section Header String Table, to hold the actual section
    // names.
    let mut shstrtab = Vec::new();
    shstrtab.push(0x00 as u8); // For the SHT_NULL section.

    // Create both 32 and 64 bit header vectors. We'll select which one to use based
    // on the container configuration, which we infer from the arch_constants
    // to determine if we're building ELF64 or ELF32.
    make_header_containers!(32, program_headers32, section_headers32);
    make_header_containers!(64, program_headers64, section_headers64);

    // Create a null section header, as required by ELF
    make_section_header!(
        64,
        goblin::elf64::section_header::SHT_NULL,
        0,
        0,
        0,
        0,
        0,
        0,
        section_headers64
    );
    make_section_header!(
        32,
        goblin::elf32::section_header::SHT_NULL,
        0,
        0,
        0,
        0,
        0,
        0,
        section_headers32
    );

    // Preallocate a vector for section data, filled with 0xFF. This pattern is chosen
    // to replicate the erase pattern we'd find in flash, and match the padding value
    // previously chosen for the objcopy gap filler.
    let mut sections_data = vec![0xFF; sections_length.try_into().unwrap()];
    let mut section_header_name_index = 0 as usize;
    // Generate all the program headers and collect all the sections together.
    for (base, sec) in sections {
        if sec.data.is_empty() {
            // Do not create a program header for an empty section. There's nothing to load.
            continue;
        }

        let this_section_base_offset = base - sections_base_address;
        let this_section_end_offset =
            this_section_base_offset as usize + sec.data.len() as usize;

        if ctx.container.is_big() {
            make_program_header!(
                64,
                this_section_base_offset,
                *base,
                sec.data.len(),
                0x20, // alignment
                program_headers64
            );
            make_section_header!(
                64,
                goblin::elf64::section_header::SHT_PROGBITS,
                (goblin::elf64::section_header::SHF_ALLOC
                    | goblin::elf64::section_header::SHF_EXECINSTR),
                shstrtab.len(),
                this_section_base_offset,
                sec.data.len(),
                *base,
                0x20,
                section_headers64
            );
        } else {
            make_program_header!(
                32,
                this_section_base_offset,
                *base,
                sec.data.len(),
                0x20, // alignment
                program_headers32
            );
            make_section_header!(
                32,
                goblin::elf32::section_header::SHT_PROGBITS,
                (goblin::elf32::section_header::SHF_ALLOC
                    | goblin::elf32::section_header::SHF_EXECINSTR),
                shstrtab.len(),
                this_section_base_offset,
                sec.data.len(),
                *base,
                0x20,
                section_headers32
            );
        }

        let task_name = sec.source_file.file_name().to_owned().unwrap();

        let mut section_name: String = String::from(".text.");
        section_name.push_str(section_header_name_index.to_string().as_str());
        section_name.push('.');
        section_name.push_str(task_name.to_str().unwrap());
        shstrtab.extend_from_slice(section_name.as_bytes());
        shstrtab.push(0x00 as u8);

        sections_data.splice(
            this_section_base_offset as usize..this_section_end_offset as usize,
            sec.data.iter().cloned(),
        );

        section_header_name_index += 1;
    }

    // We can now compute these offsets
    let program_headers_offset = goblin::elf::Header::size(ctx)
        + if ctx.container.is_big() {
            goblin::elf::ProgramHeader::size(ctx) * program_headers64.len()
        } else {
            goblin::elf::ProgramHeader::size(ctx) * program_headers32.len()
        };

    // Page align the start of sections data.
    let sections_data_offset = (program_headers_offset + 0xfff) & !0xfff;

    // Page align the Section Header String Table.
    let shstrtab_offset =
        (sections_data_offset + sections_data.len() + 0xfff) & !0xfff;

    // Add the section header for the Section Header String Table
    let shstrtab_name_offset = shstrtab.len();
    shstrtab.extend_from_slice(".shstrtab".as_bytes());
    shstrtab.push(0x00 as u8);
    shstrtab.shrink_to_fit();

    if ctx.container.is_big() {
        make_section_header!(
            64,
            goblin::elf64::section_header::SHT_STRTAB,
            0,
            shstrtab_name_offset,
            shstrtab_offset,
            shstrtab.len(),
            0,
            0,
            section_headers64
        );
    } else {
        make_section_header!(
            32,
            goblin::elf32::section_header::SHT_STRTAB,
            0,
            shstrtab_name_offset,
            shstrtab_offset,
            shstrtab.len(),
            0,
            0,
            section_headers32
        );
    }
    // Page align the Section Headers.
    let sh_data_offset = (shstrtab_offset + shstrtab.len() + 0xfff) & !0xfff;

    let shstrtab_name_offset32: usize = if section_headers32.len() > 0 {
        section_headers32.len() - 1 as usize
    } else {
        0 as usize
    };

    let shstrtab_name_offset64: usize = if section_headers64.len() > 0 {
        section_headers64.len() - 1 as usize
    } else {
        0 as usize
    };

    // Make both headers, but we'll only write out one.
    make_header!(
        32,
        le,
        header32,
        kentry,
        sh_data_offset,
        program_headers32,
        section_headers32,
        shstrtab_name_offset32,
        ctx
    );
    make_header!(
        64,
        le,
        header64,
        kentry,
        sh_data_offset,
        program_headers64,
        section_headers64,
        shstrtab_name_offset64,
        ctx
    );

    match cfg.arch_target {
        ArchTarget::ARM => {
            header32.e_machine = goblin::elf::header::EM_ARM;
            header64.e_machine = goblin::elf::header::EM_ARM;
        }
        ArchTarget::RISCV32 | ArchTarget::RISCV64 => {
            // Unlike ARM/AARCH64, RISC-V uses a single idenifier.
            header32.e_machine = goblin::elf::header::EM_RISCV;
            header64.e_machine = goblin::elf::header::EM_RISCV;
        }
    }

    // Assemble all components into the final ELF bitstream:
    // - Header
    // - Program Headers
    // - Sections Bitstream
    // - Section Header String Table
    // - Section Headers
    if ctx.container.is_big() {
        let mut elf_out = vec![
            0;
            sh_data_offset
                + goblin::elf::SectionHeader::size(ctx)
                    * section_headers64.len()
        ];
        elf_out.pwrite(header64, 0)?;

        let mut offset = goblin::elf::Header::size(ctx);
        for program_header in program_headers64 {
            elf_out.pwrite(program_header, offset)?;
            offset += goblin::elf::ProgramHeader::size(ctx);
        }

        elf_out.pwrite(sections_data.as_slice(), sections_data_offset)?;
        elf_out.pwrite(shstrtab.as_slice(), shstrtab_offset)?;

        let mut sh_offset = sh_data_offset;
        for section_header in section_headers64 {
            elf_out.pwrite(section_header, sh_offset)?;
            sh_offset += goblin::elf::SectionHeader::size(ctx);
        }

        std::fs::write(out, elf_out)?;
    } else {
        let mut elf_out = vec![
            0;
            sh_data_offset
                + goblin::elf::SectionHeader::size(ctx)
                    * section_headers32.len()
        ];
        elf_out.pwrite(header32, 0)?;

        let mut offset = goblin::elf::Header::size(ctx);
        for program_header in program_headers32 {
            elf_out.pwrite(program_header, offset)?;
            offset += goblin::elf::ProgramHeader::size(ctx);
        }

        elf_out.pwrite(sections_data.as_slice(), sections_data_offset)?;
        elf_out.pwrite(shstrtab.as_slice(), shstrtab_offset)?;

        let mut sh_offset = sh_data_offset;
        for section_header in section_headers32 {
            elf_out.pwrite(section_header, sh_offset)?;
            sh_offset += goblin::elf::SectionHeader::size(ctx);
        }
    }

    Ok(())
}

fn objcopy_translate_format(
    cmd_str: &str,
    in_format: &str,
    src: &Path,
    out_format: &str,
    dest: &Path,
) -> Result<()> {
    let mut cmd = Command::new(cmd_str);
    cmd.arg("-I")
        .arg(in_format)
        .arg("-O")
        .arg(out_format)
        .arg("--gap-fill")
        .arg("0xFF")
        .arg("--srec-forceS3") // Manually constructed Srecords use the S3 format
        .arg("--srec-len=255") // Objcopy will select a shorter line length if allowed, this forces it to match the manual Srecord construction.
        .arg(src)
        .arg(dest);

    let status = cmd
        .status()
        .context(format!("failed to objcopy ({:?})", cmd))?;

    if !status.success() {
        bail!("objcopy failed, see output for details");
    }
    Ok(())
}

fn cargo_clean(names: &[&str], target: &str) -> Result<()> {
    let mut cmd = Command::new("cargo");
    cmd.arg("clean");
    println!("cleaning {:?}", names);
    for name in names {
        cmd.arg("-p").arg(name);
    }
    cmd.arg("--release").arg("--target").arg(target);

    let status = cmd
        .status()
        .context(format!("failed to cargo clean ({:?})", cmd))?;

    if !status.success() {
        bail!("command failed, see output for details");
    }

    Ok(())
}

fn resolve_task_slots(
    cfg: &PackageConfig,
    task_name: &str,
    image_name: &str,
) -> Result<()> {
    use scroll::{Pread, Pwrite};

    let task_toml = &cfg.toml.tasks[task_name];

    let task_bin = cfg.img_file(&task_name, image_name);
    let in_task_bin = std::fs::read(&task_bin)?;
    let elf = goblin::elf::Elf::parse(&in_task_bin)?;

    let mut out_task_bin = in_task_bin.clone();

    for entry in task_slot::get_task_slot_table_entries(&in_task_bin, &elf)? {
        let in_task_idx = in_task_bin.pread_with::<u16>(
            entry.taskidx_file_offset as usize,
            elf::get_endianness(&elf),
        )?;

        let target_task_name = match task_toml.task_slots.get(entry.slot_name) {
            Some(x) => x,
            _ => bail!(
                "Program for task '{}' contains a task_slot named '{}', but it is missing from the app.toml",
                task_name,
                entry.slot_name
            ),
        };

        let target_task_idx =
            match cfg.toml.tasks.get_index_of(target_task_name) {
                Some(x) => x,
                _ => bail!(
                    "app.toml sets task '{}' task_slot '{}' to task '{}', but no such task exists in the app.toml",
                    task_name,
                    entry.slot_name,
                    target_task_name
                ),
            };

        out_task_bin.pwrite_with::<u16>(
            target_task_idx as u16,
            entry.taskidx_file_offset as usize,
            elf::get_endianness(&elf),
        )?;

        if cfg.verbose {
            println!(
                "Task '{}' task_slot '{}' changed from task index {:#x} to task index {:#x}",
                task_name, entry.slot_name, in_task_idx, target_task_idx
            );
        }
    }

    Ok(std::fs::write(task_bin, out_task_bin)?)
}
