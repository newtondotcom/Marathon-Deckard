//! Image, overlay and property mounts of the Android rootfs, run through
//! mount(8) by path. Every path must be controlled by root
//! ([`owner_controlled`]), so only root could swap one between the check and
//! the mount, and root already owns the host. Session-owned paths are bound
//! from descriptors instead (`session_config`).

use std::fs;
use std::os::unix::fs::{MetadataExt, PermissionsExt, lchown};
use std::path::{Component, Path, PathBuf};

use deckard_host_tools::files;
use deckard_host_tools::names::{USERNS_BASE, USERNS_IDS, android};

use crate::Result;
use crate::command::{self, Command};

/// Where Android's apexd finds the APEX payloads the host mounts in a user
/// namespace (system/apex 0003, kHostPayloads), each at its APEX file's path.
const HOST_APEX: &str = "deckard_apex";
/// Android's directories of pre-installed APEXes, with where each is in the
/// rootfs: system_ext and product are symlinks into system there.
const APEX_DIRS: [(&str, &str); 4] = [
    ("system/apex", "system/apex"),
    ("system_ext/apex", "system/system_ext/apex"),
    ("product/apex", "system/product/apex"),
    ("vendor/apex", "vendor/apex"),
];

#[derive(Clone, Debug)]
pub struct MountLayout {
    pub work: PathBuf,
    pub images: PathBuf,
    /// Whether `overlay/` is mounted over the images (`mount_overlays`).
    pub overlays: bool,
    /// Whether Android runs in its own user namespace (`userns`): the images
    /// and `overlay/` are then mounted idmapped into its ids, and the
    /// writable layers are kept in them on disk.
    pub userns: bool,
}

fn display(path: &Path) -> String {
    path.display().to_string()
}

impl MountLayout {
    #[must_use]
    pub fn rootfs(&self) -> PathBuf {
        self.work.join("rootfs")
    }

    /// `overlay/` as Android's user namespace sees it.
    fn overlay_idmapped(&self) -> PathBuf {
        self.work.join("overlay_idmap")
    }

    /// # Errors
    /// A directory or image is missing or not controlled by root.
    pub fn preflight(&self) -> Result<()> {
        safe_dir(&self.work)?;
        safe_dir(&self.images)?;
        for path in [
            self.images.join("system.img"),
            self.images.join("vendor.img"),
            self.work.join("deckard.prop"),
        ] {
            owner_controlled(&path)?;
            if !path.metadata()?.is_file() {
                return Err(format!("image is not a regular file: {}", path.display()).into());
            }
        }
        if self.overlays {
            safe_dir(&self.work.join("overlay"))?;
            safe_dir(&self.work.join("overlay/vendor"))?;
        }
        Ok(())
    }

    /// The mounts, in order, each with its target last.
    ///
    /// # Errors
    /// As for [`preflight`](Self::preflight), or a path is unusable in mount options.
    pub fn commands(&self) -> Result<Vec<Command>> {
        self.preflight()?;
        let root = self.rootfs();
        let mut commands = Vec::new();
        let (image_options, overlay) = if self.userns {
            let idmap = format!("X-mount.idmap=b:0:{USERNS_BASE}:{USERNS_IDS}");
            let overlay = self.overlay_idmapped();
            if self.overlays {
                commands.push(Command::new([
                    "mount",
                    "--bind",
                    "-o",
                    &idmap,
                    &display(&self.work.join("overlay")),
                    &display(&overlay),
                ]));
            }
            (format!("ro,loop,{idmap}"), overlay)
        } else {
            ("ro,loop".to_owned(), self.work.join("overlay"))
        };
        for (image, target, lower, overlay) in [
            ("system.img", root.clone(), overlay.clone(), "system"),
            (
                "vendor.img",
                root.join("vendor"),
                overlay.join("vendor"),
                "vendor",
            ),
        ] {
            let image = display(&self.images.join(image));
            commands.push(Command::new([
                "mount",
                "-o",
                &image_options,
                &image,
                &display(&target),
            ]));
            if self.overlays {
                let upper = self.work.join("overlay_rw").join(overlay);
                let work = self.work.join("overlay_work").join(overlay);
                for path in [&lower, &upper, &work, &target] {
                    validate_path_syntax(path)?;
                }
                let options = format!(
                    "lowerdir={}:{},upperdir={},workdir={},xino=off",
                    display(&lower),
                    display(&target),
                    display(&upper),
                    display(&work)
                );
                let args = [
                    "mount",
                    "-t",
                    "overlay",
                    "-o",
                    &options,
                    "overlay",
                    &display(&target),
                ];
                commands.push(Command::new(args));
            }
        }
        let prop = display(&self.work.join("deckard.prop"));
        let target = display(&root.join(android::VENDOR_PROPERTIES));
        commands.push(Command::new(["mount", "--bind", &prop, &target]));
        Ok(commands)
    }

    /// Mounts everything, unmounting what was mounted on failure.
    ///
    /// # Errors
    /// The layout is unsafe, the rootfs already holds mounts or a mount fails.
    pub fn mount(&self) -> Result<()> {
        let commands = self.commands()?;
        for path in [
            self.rootfs(),
            self.rootfs().join("vendor"),
            self.overlay_idmapped(),
            self.work.join("overlay_work/system"),
            self.work.join("overlay_work/vendor"),
        ] {
            owner_controlled(&path)?;
            fs::create_dir_all(&path)?;
            safe_dir(&path)?;
        }
        // An upper layer is the container's own tree, so in a user namespace
        // its root belongs to Android's root; its parent keeps it root's to
        // swap.
        let uppers = [
            self.work.join("overlay_rw/system"),
            self.work.join("overlay_rw/vendor"),
        ];
        for upper in &uppers {
            let parent = upper.parent().ok_or("upper layer has no parent")?;
            owner_controlled(parent)?;
            fs::create_dir_all(upper)?;
            safe_dir(parent)?;
            crate::ensure(
                fs::symlink_metadata(upper)?.is_dir(),
                format!("upper layer is not a directory: {}", upper.display()),
            )?;
        }
        let mountinfo = fs::read_to_string("/proc/self/mountinfo")?;
        if !mountpoints_beneath(&self.rootfs(), &mountinfo).is_empty()
            || !mountpoints_beneath(&self.overlay_idmapped(), &mountinfo).is_empty()
        {
            return Err("rootfs already contains mounts".into());
        }
        for upper in &uppers {
            shift_ids(upper, self.userns)?;
        }
        // apexd takes the payloads from the host whenever this exists.
        let payloads = self.work.join("overlay_rw/system").join(HOST_APEX);
        if !self.userns && fs::symlink_metadata(&payloads).is_ok() {
            fs::remove_dir_all(&payloads)?;
        }
        let mut completed = Vec::new();
        let run = |commands: Vec<Command>, completed: &mut Vec<String>| -> Result<()> {
            for command in commands {
                let target = command.args.last().cloned().ok_or("mount target missing")?;
                command::run(command)?;
                completed.push(target);
            }
            Ok(())
        };
        let mut result = run(commands, &mut completed);
        if result.is_ok() && self.userns {
            result = self
                .apex_payloads()
                .and_then(|commands| run(commands, &mut completed));
        }
        if let Err(error) = result {
            for target in completed.into_iter().rev() {
                let _ = command::run(Command::new(["umount".into(), target]));
            }
            return Err(error);
        }
        Ok(())
    }

    /// The mounts of the pre-installed APEXes' payloads, read-only and
    /// idmapped, for apexd to bind (`userns`).
    ///
    /// # Errors
    /// An APEX directory cannot be read, or an APEX is not a zip with a
    /// stored payload.
    fn apex_payloads(&self) -> Result<Vec<Command>> {
        let root = self.rootfs();
        let mut commands = Vec::new();
        for (android, host) in APEX_DIRS {
            let Some(entries) = files::found(fs::read_dir(root.join(host)))? else {
                continue;
            };
            for entry in entries {
                let path = entry?.path();
                if path.extension().is_none_or(|extension| extension != "apex") {
                    continue;
                }
                let mut apex = zip::ZipArchive::new(fs::File::open(&path)?)?;
                let payload = apex.by_name("apex_payload.img")?;
                let offset = payload
                    .data_start()
                    .ok_or("APEX payload has no data offset")?;
                crate::ensure(
                    payload.compression() == zip::CompressionMethod::Stored,
                    format!("APEX payload is compressed: {}", path.display()),
                )?;
                let name = path.file_name().ok_or("APEX without a name")?;
                let target = root.join(HOST_APEX).join(android).join(name);
                validate_path_syntax(&target)?;
                fs::create_dir_all(&target)?;
                let options = format!(
                    "ro,nodev,loop,offset={offset},sizelimit={},X-mount.idmap=b:0:{USERNS_BASE}:{USERNS_IDS}",
                    payload.size()
                );
                commands.push(Command::new([
                    "mount".to_owned(),
                    "-o".to_owned(),
                    options,
                    display(&path),
                    display(&target),
                ]));
            }
        }
        Ok(commands)
    }

    /// Whether anything is mounted beneath the rootfs.
    ///
    /// # Errors
    /// The mount table cannot be read.
    pub fn mounted(&self) -> Result<bool> {
        let mountinfo = fs::read_to_string("/proc/self/mountinfo")?;
        Ok(!mountpoints_beneath(&self.rootfs(), &mountinfo).is_empty())
    }

    /// # Errors
    /// The work directory is unsafe or an unmount fails.
    pub fn unmount(&self) -> Result<()> {
        safe_dir(&self.work)?;
        let mountinfo = fs::read_to_string("/proc/self/mountinfo")?;
        let mountpoints = mountpoints_beneath(&self.rootfs(), &mountinfo)
            .into_iter()
            .chain(mountpoints_beneath(&self.overlay_idmapped(), &mountinfo));
        for mountpoint in mountpoints {
            command::run(Command::new(["umount".into(), display(&mountpoint)]))?;
        }
        Ok(())
    }
}

/// Moves the owners of everything under `root` into Android's user
/// namespace ids (`into`) or back out. Each file is shifted alone and only
/// when it is on the other side, so an interrupted run resumes where it
/// stopped and a layer written in either mode converts.
fn shift_ids(root: &Path, into: bool) -> Result<()> {
    let shift = |id: u32| match (into, id >= USERNS_BASE) {
        (true, false) if id < USERNS_IDS => Some(id + USERNS_BASE),
        (false, true) if id - USERNS_BASE < USERNS_IDS => Some(id - USERNS_BASE),
        _ => None,
    };
    let metadata = fs::symlink_metadata(root)?;
    let (uid, gid) = (shift(metadata.uid()), shift(metadata.gid()));
    if metadata.is_dir() {
        for entry in fs::read_dir(root)? {
            shift_ids(&entry?.path(), into)?;
        }
    }
    if uid.is_some() || gid.is_some() {
        lchown(
            root,
            uid.or(Some(metadata.uid())),
            gid.or(Some(metadata.gid())),
        )?;
        // chown clears set-id bits; put the mode back.
        if !metadata.is_symlink() {
            fs::set_permissions(root, fs::Permissions::from_mode(metadata.mode() & 0o7777))?;
        }
    }
    Ok(())
}

fn validate_path_syntax(path: &Path) -> Result<()> {
    let plain = path
        .components()
        .all(|part| matches!(part, Component::RootDir | Component::Normal(_)));
    if !path.is_absolute() || !plain {
        return Err(format!("unsafe path: {}", path.display()).into());
    }
    let value = path.to_string_lossy();
    if value.contains([':', ',', '\n', '\r', '\0', '\\']) {
        return Err(format!("unsafe mount option path: {value}").into());
    }
    Ok(())
}

/// Every existing component of `path` is a real file or directory owned by
/// root or this process and not writable by group or others, except sticky
/// directories, whose entries others cannot replace. Then only root can swap
/// a component between this check and a mount.
///
/// # Errors
/// A component is a link, foreign, or writable by others.
pub(crate) fn owner_controlled(path: &Path) -> Result<()> {
    validate_path_syntax(path)?;
    let owners = [0, rustix::process::geteuid().as_raw()];
    let mut current = PathBuf::from("/");
    let mut components = path.components().skip(1).peekable();
    loop {
        let Some(metadata) = files::found(current.symlink_metadata())? else {
            return Ok(());
        };
        let shared = metadata.mode() & 0o022 != 0;
        let sticky_dir = metadata.is_dir() && metadata.mode() & 0o1000 != 0;
        if metadata.is_symlink()
            || !owners.contains(&metadata.uid())
            || (shared && !(sticky_dir && components.peek().is_some()))
        {
            let current = current.display();
            return Err(format!("mount path is not controlled by root: {current}").into());
        }
        let Some(part) = components.next() else {
            return Ok(());
        };
        current.push(part);
    }
}

fn safe_dir(path: &Path) -> Result<()> {
    owner_controlled(path)?;
    if !path.is_dir() {
        return Err(format!("directory missing: {}", path.display()).into());
    }
    Ok(())
}

/// The mount points at or below `root`, deepest and newest first.
#[must_use]
pub fn mountpoints_beneath(root: &Path, mountinfo: &str) -> Vec<PathBuf> {
    let mut entries = Vec::new();
    for line in mountinfo.lines() {
        let fields: Vec<&str> = line.split_whitespace().collect();
        let (Some(id), Some(point)) = (fields.first(), fields.get(4)) else {
            continue;
        };
        let point = PathBuf::from(point.replace("\\040", " "));
        if let Ok(id) = id.parse::<u64>()
            && point.starts_with(root)
        {
            entries.push((point.components().count(), id, point));
        }
    }
    entries.sort_by(|a, b| b.cmp(a));
    entries.into_iter().map(|(_, _, path)| path).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_overlay_metacharacters_and_symlinks() -> Result<()> {
        assert!(validate_path_syntax(Path::new("/var/lib/foo:bar")).is_err());
        assert!(validate_path_syntax(Path::new("/var/lib/foo,bar")).is_err());
        assert!(validate_path_syntax(Path::new("/var/../lib/deckard")).is_err());
        let base = tempfile::tempdir()?;
        let link = base.path().join("link");
        std::os::unix::fs::symlink("/tmp", &link)?;
        assert!(owner_controlled(&link.join("image.img")).is_err());
        Ok(())
    }

    #[test]
    fn nested_and_stacked_mounts_unmount_in_safe_order() {
        let table = "1 0 0:1 / /var/lib/deckard/rootfs rw - ext4 x y\n2 1 0:2 / /var/lib/deckard/rootfs rw - overlay x y\n3 2 0:3 / /var/lib/deckard/rootfs/vendor rw - ext4 x y\n4 3 0:4 / /var/lib/deckard/rootfs/vendor rw - overlay x y\n5 0 0:5 / /var/lib/deckard/rootfs-extra rw - ext4 x y";
        let mounts = mountpoints_beneath(Path::new("/var/lib/deckard/rootfs"), table);
        let vendor = PathBuf::from("/var/lib/deckard/rootfs/vendor");
        let root = PathBuf::from("/var/lib/deckard/rootfs");
        assert_eq!(mounts, [vendor.clone(), vendor, root.clone(), root]);
    }
}
