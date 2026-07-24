use core::fmt::Write as _;

use crate::{
    device::{
        console::ConsoleTerminalIdentity,
        devnum::{self, MINOR_BITS},
    },
    fs::devfs::{DevfsNodeAttr, DevfsNodeOps, DevfsPublish, publish as devfs_publish},
    prelude::*,
    utils::any_opaque::NilOpaque,
};

use super::{TtyEndpoint, TtyPortId, TtyWakeHandle, file, relation};

const TTY_SERIAL_MINOR_BASE: usize = 64;
const TTY_CONTROLLING_MINOR: usize = 0;

struct TtyNodeOps {
    endpoint: Weak<TtyEndpoint>,
}

impl TtyNodeOps {
    fn opened_file(&self) -> Result<OpenedFile, SysError> {
        let endpoint = self.endpoint.upgrade().ok_or(SysError::NotFound)?;
        opened_endpoint_file(&endpoint)
    }
}

impl DevfsNodeOps for TtyNodeOps {
    fn open(&self, _inode: &InodeRef) -> Result<OpenedFile, SysError> {
        self.opened_file()
    }

    fn get_attr(&self, inode: &InodeRef, attr: DevfsNodeAttr) -> Result<InodeStat, SysError> {
        Ok(endpoint_stat(inode, attr.rdev))
    }
}

struct ControllingTtyNodeOps;

impl DevfsNodeOps for ControllingTtyNodeOps {
    fn open(&self, _inode: &InodeRef) -> Result<OpenedFile, SysError> {
        let caller = crate::task::jobctl::TtyCaller::current()
            .map_err(|_| SysError::NoSuchDeviceOrAddress)?;
        let endpoint = relation::current_endpoint(&caller)?;
        opened_endpoint_file(&endpoint)
    }

    fn get_attr(&self, inode: &InodeRef, attr: DevfsNodeAttr) -> Result<InodeStat, SysError> {
        Ok(endpoint_stat(inode, attr.rdev))
    }
}

struct PreparedEndpoint {
    endpoint: Arc<TtyEndpoint>,
    devnum: CharDevNum,
    publish: DevfsPublish,
}

/// Stable boot mapping committed by the TTY owner. `devnum` is intentionally
/// retained as a stable snapshot because it is the published ABI identity;
/// the physical identity itself remains authoritative at `endpoint.port.id()`.
struct PublishedEndpoint {
    endpoint: Arc<TtyEndpoint>,
    devnum: CharDevNum,
}

struct PublishedEndpoints {
    endpoints: Vec<PublishedEndpoint>,
    /// Stable selected-endpoint snapshot used by the anonymous boot inode for
    /// inherited `fstat`/reopen behavior. The indexed endpoint remains the
    /// authoritative source of terminal identity and published device number.
    boot_index: usize,
}

static PUBLISHED_ENDPOINTS: MonoOnce<PublishedEndpoints> = unsafe { MonoOnce::new() };

fn endpoint_devnum(index: usize) -> Result<CharDevNum, SysError> {
    let minor = TTY_SERIAL_MINOR_BASE
        .checked_add(index)
        .ok_or(SysError::NoSpace)?;
    if minor >= 1 << MINOR_BITS {
        return Err(SysError::NoSpace);
    }
    Ok(CharDevNum::new(
        MajorNum::new(devnum::char::major::TTY),
        MinorNum::new(minor),
    ))
}

fn endpoint_name(index: usize) -> Result<String, SysError> {
    let mut name = String::new();
    name.try_reserve(10).map_err(|_| SysError::OutOfMemory)?;
    write!(&mut name, "ttyS{index}").expect("reserved TTY endpoint name capacity is sufficient");
    Ok(name)
}

fn select_identity_index(
    identities: &[TtyPortId],
    selected: Option<&str>,
) -> Result<usize, SysError> {
    if identities.is_empty() {
        return Err(SysError::NotFound);
    }
    match selected {
        Some(selected) => Ok(identities
            .iter()
            .position(|identity| identity.as_str() == selected)
            .unwrap_or(0)),
        None => Ok(0),
    }
}

pub(crate) struct TtyBootPublication {
    controlling_publish: DevfsPublish,
    relations: relation::PreparedRelations,
    endpoints: Vec<PreparedEndpoint>,
    published: Vec<PublishedEndpoint>,
    boot_index: usize,
    boot_files: [File; 3],
}

pub(crate) fn prepare_system_boot(
    selected: Option<&ConsoleTerminalIdentity>,
) -> Result<TtyBootPublication, SysError> {
    let port_count = super::UNPUBLISHED_PORTS.lock().len();
    let mut endpoints = Vec::new();
    let mut identities = Vec::new();
    endpoints
        .try_reserve_exact(port_count)
        .map_err(|_| SysError::OutOfMemory)?;
    identities
        .try_reserve_exact(port_count)
        .map_err(|_| SysError::OutOfMemory)?;

    let ports = super::UNPUBLISHED_PORTS.lock();
    assert_eq!(
        ports.len(),
        port_count,
        "TTY attachments changed during boot publication preparation"
    );
    for (identity, endpoint) in ports.iter() {
        assert!(
            endpoints.len() < endpoints.capacity() && identities.len() < identities.capacity(),
            "TTY attachment snapshot exceeded its reserved capacity"
        );
        let endpoint = endpoint.upgrade().ok_or(SysError::NotFound)?;
        identities.push(identity.clone());
        endpoints.push(endpoint);
    }
    drop(ports);

    let relations = relation::prepare(&endpoints)?;
    let controlling_ops: Arc<dyn DevfsNodeOps> =
        Arc::try_new(ControllingTtyNodeOps).map_err(|_| SysError::OutOfMemory)?;
    let mut controlling_name = String::new();
    controlling_name
        .try_reserve_exact(3)
        .map_err(|_| SysError::OutOfMemory)?;
    controlling_name.push_str("tty");
    let controlling_publish = DevfsPublish {
        name: controlling_name,
        attr: DevfsNodeAttr {
            ty: InodeType::Char,
            perm: InodePerm::all_rw(),
            rdev: DeviceId::Char(CharDevNum::new(
                MajorNum::new(devnum::char::major::TTY_AUX),
                MinorNum::new(TTY_CONTROLLING_MINOR),
            )),
        },
        ops: controlling_ops,
    };

    assert!(
        identities.windows(2).all(|pair| pair[0] < pair[1]),
        "TTY attachment registry lost its unique sorted identity invariant"
    );
    let selected_index =
        select_identity_index(&identities, selected.map(ConsoleTerminalIdentity::as_str))?;

    let mut prepared = Vec::new();
    prepared
        .try_reserve_exact(endpoints.len())
        .map_err(|_| SysError::OutOfMemory)?;
    for (index, endpoint) in endpoints.into_iter().enumerate() {
        let devnum = endpoint_devnum(index)?;
        let name = endpoint_name(index)?;
        let ops: Arc<dyn DevfsNodeOps> = Arc::try_new(TtyNodeOps {
            endpoint: Arc::downgrade(&endpoint),
        })
        .map_err(|_| SysError::OutOfMemory)?;
        prepared.push(PreparedEndpoint {
            endpoint,
            devnum,
            publish: DevfsPublish {
                name,
                attr: DevfsNodeAttr {
                    ty: InodeType::Char,
                    perm: InodePerm::all_rw(),
                    rdev: DeviceId::Char(devnum),
                },
                ops,
            },
        });
    }

    // This detached inode gives boot fd 0/1/2 real TTY FileOps and the selected
    // terminal's rdev without requiring `/dev` to be mounted before init.
    let boot_path = anony_new_inode(InodeType::Char, &BOOT_TTY_INODE_OPS, NilOpaque::new())?;
    let selected = &prepared[selected_index].endpoint;
    let boot_files = [
        anony_open_with(&boot_path, opened_endpoint_file(selected)?)?,
        anony_open_with(&boot_path, opened_endpoint_file(selected)?)?,
        anony_open_with(&boot_path, opened_endpoint_file(selected)?)?,
    ];
    let mut published = Vec::new();
    published
        .try_reserve_exact(prepared.len())
        .map_err(|_| SysError::OutOfMemory)?;
    for endpoint in &prepared {
        published.push(PublishedEndpoint {
            endpoint: endpoint.endpoint.clone(),
            devnum: endpoint.devnum,
        });
    }
    Ok(TtyBootPublication {
        controlling_publish,
        relations,
        endpoints: prepared,
        published,
        boot_index: selected_index,
        boot_files,
    })
}

/// Perform the boot-only single-way publication commit in deterministic order.
/// All owner-side allocations happened in `prepare_system_boot()`. Any devfs
/// error is boot-fatal at the caller; runtime unpublish/renumber is
/// deliberately outside the first-version protocol.
impl TtyBootPublication {
    pub(crate) fn publish(self) -> Result<[File; 3], SysError> {
        let Self {
            controlling_publish,
            relations,
            endpoints,
            published,
            boot_index,
            boot_files,
        } = self;

        relation::install(relations);
        devfs_publish(controlling_publish)?;

        for prepared in endpoints {
            assert_eq!(
                prepared.publish.attr.rdev,
                DeviceId::Char(prepared.devnum),
                "TTY name/devnum publication snapshot diverged"
            );
            kinfoln!(
                "TTY: publishing /dev/{} as {} for {}",
                prepared.publish.name,
                prepared.devnum,
                prepared.endpoint.port.id()
            );
            devfs_publish(prepared.publish)?;
        }

        assert!(
            boot_index < published.len(),
            "TTY boot selection escaped its published endpoint snapshot"
        );
        PUBLISHED_ENDPOINTS.init(|slot| {
            slot.write(PublishedEndpoints {
                endpoints: published,
                boot_index,
            });
        });
        Ok(boot_files)
    }
}

fn selected_endpoint() -> Result<&'static PublishedEndpoint, SysError> {
    let registry = PUBLISHED_ENDPOINTS.get();
    registry
        .endpoints
        .get(registry.boot_index)
        .ok_or(SysError::NotFound)
}

fn opened_endpoint_file(endpoint: &Arc<TtyEndpoint>) -> Result<OpenedFile, SysError> {
    let wake_source = endpoint.wake_source.upgrade().ok_or(SysError::NotFound)?;
    Ok(file::opened_file(
        endpoint.clone(),
        TtyWakeHandle {
            source: wake_source,
        },
    ))
}

fn selected_opened_file() -> Result<OpenedFile, SysError> {
    opened_endpoint_file(&selected_endpoint()?.endpoint)
}

fn endpoint_stat(inode: &InodeRef, rdev: DeviceId) -> InodeStat {
    InodeStat {
        fs_dev: DeviceId::None,
        ino: inode.ino(),
        mode: InodeMode::new(InodeType::Char, inode.perm()),
        nlink: inode.nlink(),
        uid: inode.uid(),
        gid: inode.gid(),
        rdev,
        size: inode.size(),
        atime: inode.atime(),
        mtime: inode.mtime(),
        ctime: inode.ctime(),
    }
}

fn boot_tty_get_attr(inode: &InodeRef) -> Result<InodeStat, SysError> {
    Ok(endpoint_stat(
        inode,
        DeviceId::Char(selected_endpoint()?.devnum),
    ))
}

static BOOT_TTY_INODE_OPS: InodeOps = InodeOps {
    lookup: |_, _| Err(SysError::NotSupported),
    touch: |_, _, _| Err(SysError::NotSupported),
    mkdir: |_, _, _| Err(SysError::NotSupported),
    symlink: |_, _, _| Err(SysError::NotSupported),
    unlink: |_, _| Err(SysError::NotSupported),
    rmdir: |_, _| Err(SysError::NotSupported),
    rename: |_, _, _, _, _| Err(SysError::NotSupported),
    link: |_, _, _| Err(SysError::NotSupported),
    truncate: |_, _| Err(SysError::NotSupported),
    get_attr: boot_tty_get_attr,
    read_link: |_| Err(SysError::NotSymlink),
    open: |_| selected_opened_file(),
};

#[cfg(feature = "kunit")]
mod kunits {
    use super::*;

    #[kunit]
    fn deterministic_identity_mapping_and_selection() {
        let mut identities = [
            TtyPortId::try_from("/soc/serial@3000").unwrap(),
            TtyPortId::try_from("/soc/serial@1000").unwrap(),
            TtyPortId::try_from("/soc/serial@2000").unwrap(),
        ];
        identities.sort();

        assert_eq!(identities[0].as_str(), "/soc/serial@1000");
        assert_eq!(endpoint_name(0).unwrap(), "ttyS0");
        assert_eq!(endpoint_name(2).unwrap(), "ttyS2");
        assert_eq!(
            endpoint_devnum(0).unwrap(),
            CharDevNum::new(
                MajorNum::new(devnum::char::major::TTY),
                MinorNum::new(TTY_SERIAL_MINOR_BASE)
            )
        );
        assert_eq!(
            select_identity_index(&identities, Some("/soc/serial@2000")),
            Ok(1)
        );
        assert_eq!(select_identity_index(&identities, None), Ok(0));
        assert_eq!(
            select_identity_index(&identities, Some("/soc/serial@4000")),
            Ok(0)
        );
    }

    #[kunit]
    fn endpoint_minor_overflow_is_rejected_before_publication() {
        assert_eq!(
            endpoint_devnum((1 << MINOR_BITS) - TTY_SERIAL_MINOR_BASE - 1)
                .unwrap()
                .minor(),
            MinorNum::new((1 << MINOR_BITS) - 1)
        );
        assert_eq!(
            endpoint_devnum((1 << MINOR_BITS) - TTY_SERIAL_MINOR_BASE),
            Err(SysError::NoSpace)
        );
    }
}
