//! The container's device cgroup: Android may open and create only the
//! nodes its config binds, the pseudo-devices every Linux system has, and
//! the loop devices the host has not attached, which apexd takes for
//! APEXes. Host disks, device-mapper, the modem and every other node stay
//! closed to it, even to its root.
//!
//! In a user namespace (`userns`) Android can neither create nodes nor
//! change the host's, so the host makes Android its own copies on a tmpfs
//! and binds them idmapped over the host's.

use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::fs;
use std::os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt, chown};
use std::path::{Path, PathBuf};

use deckard_host_tools::names::{RUN, USERNS_BASE, USERNS_IDS};
use rustix::fs::{CWD, FileType, Mode, major, makedev, minor, mknodat};

use crate::Result;
use crate::command::{self, Command};
use crate::mounts;

/// null, zero, full, random, urandom, kmsg, tty, ptmx, the pty slaves and
/// loop-control.
const PSEUDO: [&str; 10] = [
    "c 1:3", "c 1:5", "c 1:7", "c 1:8", "c 1:9", "c 1:11", "c 5:0", "c 5:2", "c 136:*", "c 10:237",
];
const LOOP_MAJOR: u32 = 7;
const LOOP_MINORS: u32 = 256;
/// The nodes Android's init makes for itself, with its modes.
const OWN_NODES: [(&str, u32, u32, u32); 8] = [
    ("null", 1, 3, 0o666),
    ("zero", 1, 5, 0o666),
    ("full", 1, 7, 0o666),
    ("random", 1, 8, 0o666),
    ("urandom", 1, 9, 0o666),
    ("kmsg", 1, 11, 0o600),
    ("kmsg_debug", 1, 11, 0o622),
    ("tty", 5, 0, 0o666),
];

/// A device node the container gets: its path in `/dev`, type, number and mode.
struct Node {
    target: String,
    kind: char,
    major: u32,
    minor: u32,
    mode: u32,
}

/// Writes the instance's `config_devices` for the host as it is now and
/// returns its path.
///
/// # Errors
/// The instance's `config_nodes` or `/sys/block` cannot be read, or the
/// file cannot be written.
pub fn write(instance: &Path, userns: bool) -> Result<PathBuf> {
    let config = fs::read_to_string(instance.join("config_nodes"))?;
    let mut bound = Vec::new();
    for (source, target) in bound_sources(&config) {
        let Ok(metadata) = fs::metadata(source) else {
            continue;
        };
        let kind = match metadata.file_type() {
            kind if kind.is_char_device() => 'c',
            kind if kind.is_block_device() => 'b',
            _ => continue,
        };
        bound.push(Node {
            target: target.to_owned(),
            kind,
            major: major(metadata.rdev()),
            minor: minor(metadata.rdev()),
            mode: metadata.mode() & 0o7777,
        });
    }
    let mut text = render(&bound, &attached_loops(Path::new("/sys/block"))?);
    if userns {
        let own = OWN_NODES.map(|(name, major, minor, mode)| Node {
            target: format!("dev/{name}"),
            kind: 'c',
            major,
            minor,
            mode,
        });
        text.push_str(&stage_nodes(
            &Path::new(RUN).join("dev"),
            own.iter().chain(&bound),
        )?);
    }
    // LXC opens the staged sources as the container's root, which may only
    // pass through them.
    let (group, mode) = if userns {
        (USERNS_BASE, 0o710)
    } else {
        (0, 0o700)
    };
    for directory in [Path::new(RUN).to_owned(), Path::new(RUN).join("session")] {
        if fs::symlink_metadata(&directory).is_ok_and(|metadata| metadata.is_dir()) {
            chown(&directory, Some(0), Some(group))?;
            fs::set_permissions(&directory, PermissionsExt::from_mode(mode))?;
        }
    }
    let path = instance.join("config_devices");
    fs::write(&path, text)?;
    Ok(path)
}

/// The `/dev` sources of `config_nodes`' mount entries, with their targets.
fn bound_sources(nodes: &str) -> impl Iterator<Item = (&str, &str)> {
    nodes.lines().filter_map(|line| {
        let mut fields = line.strip_prefix("lxc.mount.entry = ")?.split(' ');
        let (source, target) = (fields.next()?, fields.next()?);
        source.starts_with("/dev/").then_some((source, target))
    })
}

/// Makes `nodes` afresh on a tmpfs at `stage`, root's and so the container
/// root's through the idmap, and returns the config that maps the
/// container's ids and binds the nodes.
fn stage_nodes<'a>(stage: &Path, nodes: impl Iterator<Item = &'a Node>) -> Result<String> {
    mounts::owner_controlled(stage)?;
    fs::create_dir_all(stage)?;
    let mountinfo = fs::read_to_string("/proc/self/mountinfo")?;
    if mounts::mountpoints_beneath(stage, &mountinfo).is_empty() {
        let stage = stage.display().to_string();
        let tmpfs = [
            "mount",
            "-t",
            "tmpfs",
            "-o",
            "mode=0755,size=64k",
            "tmpfs",
            &stage,
        ];
        command::run(Command::new(tmpfs))?;
    }
    let mut text = format!(
        "lxc.idmap = u 0 {USERNS_BASE} {USERNS_IDS}\nlxc.idmap = g 0 {USERNS_BASE} {USERNS_IDS}\n"
    );
    for node in nodes {
        let name = node.target.trim_start_matches("dev/").replace('/', "_");
        let path = stage.join(&name);
        let _ = fs::remove_file(&path);
        let kind = if node.kind == 'b' {
            FileType::BlockDevice
        } else {
            FileType::CharacterDevice
        };
        mknodat(
            CWD,
            &path,
            kind,
            Mode::from_raw_mode(0),
            makedev(node.major, node.minor),
        )?;
        fs::set_permissions(&path, PermissionsExt::from_mode(node.mode))?;
        let _ = writeln!(
            text,
            "lxc.mount.entry = {} {} none bind,create=file,idmap=container 0 0",
            path.display(),
            node.target
        );
    }
    Ok(text)
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

fn render(bound: &[Node], attached: &BTreeSet<u32>) -> String {
    let mut text = String::from("lxc.cgroup2.devices.deny = a\n");
    let mut allow = |rule: &str| {
        let _ = writeln!(text, "lxc.cgroup2.devices.allow = {rule} rwm");
    };
    for rule in PSEUDO {
        allow(rule);
    }
    for node in bound {
        allow(&format!("{} {}:{}", node.kind, node.major, node.minor));
    }
    for minor in (0..LOOP_MINORS).filter(|minor| !attached.contains(minor)) {
        allow(&format!("b {LOOP_MAJOR}:{minor}"));
    }
    text
}
