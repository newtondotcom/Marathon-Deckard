//! The container's device cgroup: Android may open and create only the
//! nodes its config binds, the pseudo-devices every Linux system has, and
//! the loop devices the host has not attached, which apexd takes for
//! APEXes. Host disks, device-mapper, the modem and every other node stay
//! closed to it, even to its root.

use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::fs;
use std::os::unix::fs::{FileTypeExt, MetadataExt};
use std::path::{Path, PathBuf};

use rustix::fs::{major, minor};

use crate::Result;

/// null, zero, full, random, urandom, kmsg, tty, ptmx, the pty slaves and
/// loop-control.
const PSEUDO: [&str; 10] = [
    "c 1:3", "c 1:5", "c 1:7", "c 1:8", "c 1:9", "c 1:11", "c 5:0", "c 5:2", "c 136:*", "c 10:237",
];
const LOOP_MAJOR: u32 = 7;
const LOOP_MINORS: u32 = 256;

/// Writes the instance's `config_devices` for the host as it is now and
/// returns its path.
///
/// # Errors
/// The instance's `config_nodes` or `/sys/block` cannot be read, or the
/// file cannot be written.
pub fn write(instance: &Path) -> Result<PathBuf> {
    let nodes = fs::read_to_string(instance.join("config_nodes"))?;
    let mut bound = Vec::new();
    for source in bound_sources(&nodes) {
        let Ok(metadata) = fs::metadata(source) else {
            continue;
        };
        let kind = match metadata.file_type() {
            kind if kind.is_char_device() => 'c',
            kind if kind.is_block_device() => 'b',
            _ => continue,
        };
        bound.push((kind, major(metadata.rdev()), minor(metadata.rdev())));
    }
    let path = instance.join("config_devices");
    fs::write(
        &path,
        render(&bound, &attached_loops(Path::new("/sys/block"))?),
    )?;
    Ok(path)
}

/// The `/dev` sources of `config_nodes`' mount entries.
fn bound_sources(nodes: &str) -> impl Iterator<Item = &str> {
    nodes
        .lines()
        .filter_map(|line| line.strip_prefix("lxc.mount.entry = ")?.split(' ').next())
        .filter(|source| source.starts_with("/dev/"))
}

/// The minors of the loop devices bound to a file.
fn attached_loops(sys_block: &Path) -> Result<BTreeSet<u32>> {
    let mut minors = BTreeSet::new();
    for entry in fs::read_dir(sys_block)? {
        let path = entry?.path();
        if !path.join("loop/backing_file").exists() {
            continue;
        }
        let dev = fs::read_to_string(path.join("dev"))?;
        if let Some(("7", minor)) = dev.trim().split_once(':') {
            minors.insert(minor.parse()?);
        }
    }
    Ok(minors)
}

fn render(bound: &[(char, u32, u32)], attached: &BTreeSet<u32>) -> String {
    let mut text = String::from("lxc.cgroup2.devices.deny = a\n");
    let mut allow = |rule: &str| {
        let _ = writeln!(text, "lxc.cgroup2.devices.allow = {rule} rwm");
    };
    for rule in PSEUDO {
        allow(rule);
    }
    for (kind, major, minor) in bound {
        allow(&format!("{kind} {major}:{minor}"));
    }
    for minor in (0..LOOP_MINORS).filter(|minor| !attached.contains(minor)) {
        allow(&format!("b {LOOP_MAJOR}:{minor}"));
    }
    text
}
