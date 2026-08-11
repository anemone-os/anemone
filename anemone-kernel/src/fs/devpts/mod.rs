//! System devpts instance and Unix98 PTY allocation transaction.

use crate::{
    device::{
        devnum::{DeviceNumber, MINOR_BITS, MajorNum, MinorNum},
        tty::{
            LivePtyPair, PreparedPtyPair, PtyBindingCapability, PtyBindingOps, TtyLineSnapshot,
            TtyParity, prepare_pair,
        },
    },
    fs::{
        devfs::{self, DevfsNodeAttr, DevfsNodeOps, DevfsPublish},
        filesystem::FileSystemMountOps,
        inode::Inode,
        register_filesystem,
        superblock::{FsMagic, FsStat, SuperBlockOps},
    },
    prelude::*,
    utils::any_opaque::AnyOpaque,
};

mod file;
mod inode;

use inode::{DevptsInode, new_root_inode, new_slave_inode};

const DEVPTS_ROOT_INO: Ino = Ino::new(1);
const DEVPTS_SLAVE_MAJOR: usize = 136;
const DEVPTS_SUPER_MAGIC: u64 = 0x1cd1;

static_assert!(
    PTY_SYSTEM_CAPACITY > 0,
    "PTY system capacity must be non-zero"
);
static_assert!(
    PTY_SYSTEM_CAPACITY <= (1usize << MINOR_BITS),
    "PTY system capacity must fit the device minor namespace"
);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct EpisodeId {
    index: usize,
    generation: u64,
    ino: Ino,
}

enum EpisodeSlot {
    Free,
    Reserved {
        generation: u64,
    },
    Live {
        generation: u64,
        binding: Weak<DevptsBinding>,
    },
}

struct DevptsRegistry {
    slots: Vec<EpisodeSlot>,
    next_generation: u64,
    next_ino: u64,
}

struct DevptsCore {
    registry: SpinLock<DevptsRegistry>,
    /// Exact retired inode identities awaiting a later VFS eviction retry.
    /// Entries are no longer discoverable and never drive binding behavior;
    /// this queue only closes physical cache reclamation after stale PathRefs
    /// release their VFS-owned active references.
    retired_inodes: SpinLock<Vec<Arc<Inode>>>,
}

impl DevptsCore {
    fn try_new(capacity: usize) -> Result<Arc<Self>, SysError> {
        if capacity == 0 || capacity > (1usize << MINOR_BITS) {
            return Err(SysError::InvalidArgument);
        }
        let mut slots = Vec::new();
        slots
            .try_reserve_exact(capacity)
            .map_err(|_| SysError::OutOfMemory)?;
        slots.resize_with(capacity, || EpisodeSlot::Free);
        Arc::try_new(Self {
            registry: SpinLock::new(DevptsRegistry {
                slots,
                next_generation: 1,
                next_ino: DEVPTS_ROOT_INO.get() + 1,
            }),
            retired_inodes: SpinLock::new(Vec::new()),
        })
        .map_err(|_| SysError::OutOfMemory)
    }

    fn reserve(self: &Arc<Self>) -> Result<EpisodeReservation, SysError> {
        self.retry_retired_inode_eviction();
        let episode = {
            let mut registry = self.registry.lock();
            let index = registry
                .slots
                .iter()
                .position(|slot| matches!(slot, EpisodeSlot::Free))
                .ok_or(SysError::NoSpace)?;
            let generation = registry.next_generation;
            registry.next_generation = generation
                .checked_add(1)
                .expect("devpts episode generation overflow");
            let ino = Ino::try_new(registry.next_ino).map_err(|_| SysError::NoSpace)?;
            registry.next_ino = registry.next_ino.checked_add(1).ok_or(SysError::NoSpace)?;
            registry.slots[index] = EpisodeSlot::Reserved { generation };
            EpisodeId {
                index,
                generation,
                ino,
            }
        };
        Ok(EpisodeReservation {
            core: self.clone(),
            episode,
            released: AtomicBool::new(false),
        })
    }

    fn publish(&self, episode: EpisodeId, binding: &Arc<DevptsBinding>) {
        let mut registry = self.registry.lock();
        let slot = &mut registry.slots[episode.index];
        assert!(
            matches!(slot, EpisodeSlot::Reserved { generation } if *generation == episode.generation),
            "devpts binding publication lost its exact reservation"
        );
        *slot = EpisodeSlot::Live {
            generation: episode.generation,
            binding: Arc::downgrade(binding),
        };
    }

    fn release(&self, episode: EpisodeId) {
        let mut registry = self.registry.lock();
        let slot = &mut registry.slots[episode.index];
        let exact = match slot {
            EpisodeSlot::Reserved { generation } => *generation == episode.generation,
            EpisodeSlot::Live {
                generation,
                binding: _,
            } => *generation == episode.generation,
            EpisodeSlot::Free => false,
        };
        if exact {
            *slot = EpisodeSlot::Free;
        }
    }

    fn binding(&self, index: usize) -> Option<Arc<DevptsBinding>> {
        let registry = self.registry.lock();
        match registry.slots.get(index)? {
            EpisodeSlot::Live { binding, .. } => binding.upgrade(),
            EpisodeSlot::Free | EpisodeSlot::Reserved { .. } => None,
        }
    }

    fn live_entries(&self) -> Vec<(usize, Ino)> {
        let bindings: Vec<_> = {
            let registry = self.registry.lock();
            registry
                .slots
                .iter()
                .enumerate()
                .filter_map(|(index, slot)| match slot {
                    EpisodeSlot::Live { binding, .. } => Some((index, binding.clone())),
                    EpisodeSlot::Free | EpisodeSlot::Reserved { .. } => None,
                })
                .collect()
        };
        bindings
            .into_iter()
            .filter_map(|(index, binding)| {
                binding
                    .upgrade()
                    .and_then(|binding| binding.active_ino().map(|ino| (index, ino)))
            })
            .collect()
    }

    fn retire_inode(&self, inode: Arc<Inode>) {
        let sb = inode.sb();
        sb.unindex_inode(&inode);
        match sb.try_evict_inode(&inode) {
            Ok(()) | Err(SysError::NotFound) => {},
            Err(SysError::Busy) => self.retired_inodes.lock().push(inode),
            Err(err) => {
                knoticeln!(
                    "devpts: deferred retired inode eviction ino={:?} error={:?}",
                    inode.ino(),
                    err
                );
                self.retired_inodes.lock().push(inode);
            },
        }
    }

    fn retry_retired_inode_eviction(&self) {
        let mut pending = {
            let mut queue = self.retired_inodes.lock();
            core::mem::take(&mut *queue)
        };
        pending.retain(|inode| {
            let sb = inode.sb();
            match sb.try_evict_inode(inode) {
                Ok(()) | Err(SysError::NotFound) => false,
                Err(SysError::Busy) => true,
                Err(err) => {
                    knoticeln!(
                        "devpts: retired inode eviction retry ino={:?} error={:?}",
                        inode.ino(),
                        err
                    );
                    true
                },
            }
        });
        if !pending.is_empty() {
            // A concurrent retirement may already have queued another exact
            // inode. Both batches are behaviorally dead and can be merged in
            // any order; no binding or reuse decision reads this queue.
            self.retired_inodes.lock().append(&mut pending);
        }
    }

    #[cfg(feature = "kunit")]
    fn used(&self) -> usize {
        self.registry
            .lock()
            .slots
            .iter()
            .filter(|slot| !matches!(slot, EpisodeSlot::Free))
            .count()
    }

    #[cfg(feature = "kunit")]
    fn retired_inode_count(&self) -> usize {
        self.retired_inodes.lock().len()
    }
}

struct EpisodeReservation {
    core: Arc<DevptsCore>,
    episode: EpisodeId,
    released: AtomicBool,
}

impl EpisodeReservation {
    fn publish(&self, binding: &Arc<DevptsBinding>) {
        assert!(!self.released.load(Ordering::Acquire));
        self.core.publish(self.episode, binding);
    }

    fn release(&self) {
        if !self.released.swap(true, Ordering::AcqRel) {
            self.core.release(self.episode);
        }
    }
}

impl Drop for EpisodeReservation {
    fn drop(&mut self) {
        self.release();
    }
}

struct BindingProjection {
    inode: Option<Arc<Inode>>,
    published: bool,
    retired: bool,
}

struct DevptsBinding {
    reservation: EpisodeReservation,
    pair: LivePtyPair,
    peer_mount: Arc<Mount>,
    projection: SpinLock<BindingProjection>,
}

impl DevptsBinding {
    fn prepare(
        instance: &DevptsInstance,
        reservation: EpisodeReservation,
        pair: LivePtyPair,
        uid: Uid,
        gid: Gid,
    ) -> Result<Arc<Self>, SysError> {
        let binding = Arc::try_new(Self {
            reservation,
            pair,
            peer_mount: instance.root_mount.clone(),
            projection: SpinLock::new(BindingProjection {
                inode: None,
                published: false,
                retired: false,
            }),
        })
        .map_err(|_| SysError::OutOfMemory)?;
        let inode = new_slave_inode(instance.sb.clone(), Arc::downgrade(&binding), uid, gid)?;
        let inode = instance.sb.try_seed_inode(inode)?;
        binding.projection.lock().inode = Some(inode.inode().clone());
        // The resident inode is the mount-neutral VFS projection. Do not pin
        // a non-root InodeRef or Dentry here: backend binding lifetime must not
        // make the last user mount view spuriously busy.
        drop(inode);
        Ok(binding)
    }

    fn episode(&self) -> EpisodeId {
        self.reservation.episode
    }

    fn pair(&self) -> LivePtyPair {
        self.pair.clone()
    }

    fn publish(binding: &Arc<Self>) {
        {
            let mut projection = binding.projection.lock();
            assert!(!projection.retired && !projection.published);
            assert!(projection.inode.is_some());
            projection.published = true;
        }
        binding.reservation.publish(binding);
    }

    fn active_inode(&self) -> Option<InodeRef> {
        let projection = self.projection.lock();
        (projection.published && !projection.retired).then(|| {
            InodeRef::new(
                projection
                    .inode
                    .as_ref()
                    .expect("published devpts binding missing inode")
                    .clone(),
            )
        })
    }

    fn is_active(&self) -> bool {
        let projection = self.projection.lock();
        projection.published && !projection.retired
    }

    fn active_ino(&self) -> Option<Ino> {
        let projection = self.projection.lock();
        (projection.published && !projection.retired).then(|| {
            projection
                .inode
                .as_ref()
                .expect("published devpts binding missing inode")
                .ino()
        })
    }

    fn retire_inner(&self) {
        let inode = {
            let mut projection = self.projection.lock();
            if projection.retired {
                return;
            }
            projection.retired = true;
            projection.published = false;
            projection.inode.take()
        };
        // Remove backend discoverability before touching VFS residency. A
        // racing old PathRef may survive, but its weak episode capability can
        // only reach this already-retired binding and therefore fails closed.
        self.reservation.release();
        let Some(inode) = inode else {
            return;
        };
        self.reservation.core.retire_inode(inode);
    }
}

impl PtyBindingOps for DevptsBinding {
    fn retire(&self) {
        self.retire_inner();
    }

    fn peer_path(&self) -> Result<PathRef, SysError> {
        let inode = self.active_inode().ok_or(SysError::IO)?;
        let dentry = Arc::try_new(Dentry::new(
            self.episode().index.to_string(),
            Some(self.peer_mount.root().clone()),
            inode,
        ))
        .map_err(|_| SysError::OutOfMemory)?;
        Ok(PathRef::new(self.peer_mount.clone(), dentry))
    }
}

impl Drop for DevptsBinding {
    fn drop(&mut self) {
        self.retire_inner();
    }
}

#[derive(Opaque)]
struct DevptsSb {
    core: Arc<DevptsCore>,
}

struct DevptsInstance {
    core: Arc<DevptsCore>,
    sb: Arc<SuperBlock>,
    /// Detached mount-neutral projection used only to back File objects opened
    /// through `TIOCGPTPEER`; it is not a user view or mount-count truth.
    root_mount: Arc<Mount>,
}

impl DevptsInstance {
    fn try_new(fs: Arc<FileSystem>, capacity: usize) -> Result<Self, SysError> {
        let core = DevptsCore::try_new(capacity)?;
        let sb = Arc::try_new(SuperBlock::new(
            fs,
            &DEVPTS_SB_OPS,
            AnyOpaque::new(DevptsSb { core: core.clone() }),
            DEVPTS_ROOT_INO,
            MountSource::Pseudo,
        ))
        .map_err(|_| SysError::OutOfMemory)?;
        let root = sb.try_seed_inode(new_root_inode(sb.clone())?)?;
        let root_dentry = Arc::try_new(Dentry::new("/".to_string(), None, root))
            .map_err(|_| SysError::OutOfMemory)?;
        let root_mount = Arc::try_new(Mount::new(root_dentry, sb.clone(), MountAttrFlags::empty()))
            .map_err(|_| SysError::OutOfMemory)?;
        Ok(Self {
            core,
            sb,
            root_mount,
        })
    }

    fn prepare_ptmx(&self) -> Result<OpenedFile, SysError> {
        let checker = FsPermChecker::for_current_fs();
        let reservation = self.core.reserve()?;
        let index =
            u32::try_from(reservation.episode.index).map_err(|_| SysError::InvalidArgument)?;
        let mut pair = prepare_pair(
            index,
            TtyLineSnapshot {
                baud: 38400,
                parity: TtyParity::None,
                data_bits: 8,
            },
        )?;
        let binding = DevptsBinding::prepare(
            self,
            reservation,
            pair.pair_handle(),
            checker.fsuid(),
            checker.fsgid(),
        )?;
        let opened = pair.take_opened_master();
        let OpenedFile {
            file_ops,
            mode,
            prv,
            description_activation: _,
        } = opened;
        Ok(OpenedFile::with_description_activation(
            file_ops,
            mode,
            prv,
            OpenDescriptionActivation::new(
                AnyOpaque::new(PtmxActivation {
                    pair: SpinLock::new(Some(pair)),
                    binding,
                }),
                prepare_ptmx_description,
            ),
        ))
    }
}

#[derive(Opaque)]
struct PtmxActivation {
    pair: SpinLock<Option<PreparedPtyPair>>,
    binding: Arc<DevptsBinding>,
}

#[derive(Opaque)]
struct PtmxCommit {
    pair: SpinLock<Option<PreparedPtyPair>>,
    binding: Arc<DevptsBinding>,
}

impl Drop for PtmxCommit {
    fn drop(&mut self) {
        let Some(mut pair) = self.pair.lock().take() else {
            return;
        };
        // Cleanup installation creates the static live-release graph before
        // pair commit. An aborted opened-description prepare must dismantle
        // that graph explicitly so the unpublished binding can roll back.
        pair.abort_installed_cleanup();
    }
}

fn prepare_ptmx_description(
    state: AnyOpaque,
    _request: FileOpenRequest,
    description_ops: crate::task::files::FileDescOps,
) -> Result<PreparedOpenDescription, SysError> {
    let state = state
        .cast::<PtmxActivation>()
        .expect("devpts ptmx activation type mismatch");
    let mut pair = state
        .pair
        .lock()
        .take()
        .expect("devpts ptmx activation consumed more than once");
    pair.install_cleanup(PtyBindingCapability::new(state.binding.clone()))?;
    let description_ops = pair.compose_description_ops(description_ops);
    Ok(PreparedOpenDescription {
        description_ops,
        commit: OpenDescriptionCommit::new(
            AnyOpaque::new(PtmxCommit {
                pair: SpinLock::new(Some(pair)),
                binding: state.binding.clone(),
            }),
            commit_ptmx_description,
        ),
    })
}

fn commit_ptmx_description(
    state: AnyOpaque,
    description: Arc<crate::task::files::FileDesc>,
) -> Result<(), SysError> {
    let state = state
        .cast::<PtmxCommit>()
        .expect("devpts ptmx commit type mismatch");
    let pair = state
        .pair
        .lock()
        .take()
        .expect("devpts ptmx commit consumed more than once");
    pair.commit(description, |_, _| DevptsBinding::publish(&state.binding));
    Ok(())
}

fn devpts_sb(sb: &SuperBlock) -> &DevptsSb {
    sb.prv()
        .cast::<DevptsSb>()
        .expect("devpts superblock private state mismatch")
}

fn devpts_load_inode(_sb: &Arc<SuperBlock>, _ino: Ino) -> Result<Arc<Inode>, SysError> {
    Err(SysError::NotFound)
}

fn devpts_evict_inode(_inode: Arc<Inode>) -> Result<(), SysError> {
    Ok(())
}

fn devpts_sync_inode(_inode: &InodeRef) -> Result<(), SysError> {
    Ok(())
}

fn devpts_stat(_sb: &SuperBlock) -> Result<FsStat, SysError> {
    Ok(FsStat::pseudo(FsMagic::new(DEVPTS_SUPER_MAGIC)))
}

static DEVPTS_SB_OPS: SuperBlockOps = SuperBlockOps {
    load_inode: devpts_load_inode,
    evict_inode: devpts_evict_inode,
    sync_inode: devpts_sync_inode,
    stat: devpts_stat,
};

fn devpts_mount(data: MountData) -> Result<Arc<SuperBlock>, SysError> {
    data.reject_nonempty_for("devpts")?;
    Ok(system_instance().sb.clone())
}

fn devpts_sync_fs(_sb: &SuperBlock) -> Result<(), SysError> {
    Ok(())
}

fn devpts_kill_sb(_sb: Arc<SuperBlock>) {
    panic!("persistent system devpts superblock must not be killed")
}

static DEVPTS_FS_OPS: FileSystemOps = FileSystemOps {
    name: "devpts",
    flags: FileSystemFlags::PERSISTENT_SB.union(FileSystemFlags::SHRINKABLE_ICACHE),
    mount: FileSystemMountOps::NoDevice(devpts_mount),
    sync_fs: devpts_sync_fs,
    kill_sb: devpts_kill_sb,
};

static DEVPTS_INSTANCE: MonoOnce<DevptsInstance> = unsafe { MonoOnce::new() };

fn system_instance() -> &'static DevptsInstance {
    DEVPTS_INSTANCE.get()
}

struct PtmxNodeOps;

impl DevfsNodeOps for PtmxNodeOps {
    fn open(&self, _inode: &InodeRef) -> Result<OpenedFile, SysError> {
        system_instance().prepare_ptmx()
    }

    fn get_attr(&self, inode: &InodeRef, attr: DevfsNodeAttr) -> Result<InodeStat, SysError> {
        let meta = inode.inode().meta_snapshot();
        Ok(InodeStat {
            fs_dev: DeviceId::None,
            ino: inode.ino(),
            mode: InodeMode::new(attr.ty, meta.perm),
            nlink: meta.nlink,
            uid: meta.uid,
            gid: meta.gid,
            rdev: attr.rdev,
            size: 0,
            atime: meta.atime,
            mtime: meta.mtime,
            ctime: meta.ctime,
        })
    }
}

fn ptmx_publication() -> DevfsPublish {
    DevfsPublish {
        name: "ptmx".to_string(),
        attr: DevfsNodeAttr {
            ty: InodeType::Char,
            perm: InodePerm::all_rw(),
            rdev: DeviceId::Number(DeviceNumber::new(MajorNum::new(5), MinorNum::new(2))),
        },
        ops: Arc::new(PtmxNodeOps),
    }
}

/// Publish the static devfs side of the public PTY activation.
///
/// This runs only after every fs initcall has returned, so it depends on the
/// initialized devfs namespace explicitly rather than on sibling-initcall
/// link order. Publishing the empty mountpoint first also prevents a usable
/// `/dev/ptmx` from escaping when the boot-fatal namespace transaction fails.
pub(super) fn activate_public_namespace() {
    devfs::root_directory()
        .publish_directory("pts".to_string())
        .unwrap_or_else(|err| panic!("failed to publish /dev/pts mountpoint: {:?}", err));
    devfs::publish(ptmx_publication())
        .unwrap_or_else(|err| panic!("failed to publish /dev/ptmx: {:?}", err));
}

#[initcall(fs)]
fn init() {
    let fs = register_filesystem(&DEVPTS_FS_OPS)
        .unwrap_or_else(|err| panic!("failed to register devpts: {:?}", err));
    let instance = DevptsInstance::try_new(fs, PTY_SYSTEM_CAPACITY)
        .unwrap_or_else(|err| panic!("failed to initialize system devpts instance: {:?}", err));
    DEVPTS_INSTANCE.init(|slot| {
        slot.write(instance);
    });
}

#[cfg(feature = "kunit")]
mod kunits {
    use super::*;
    use crate::{
        fs::{
            FileOpenAccess, FileOpenRequest, IoctlArgFdLookup, IoctlArgFile, IoctlCtx,
            fanotify::observed_file_description_ops, get_filesystem, mount::MountTree,
            vfs_mkdir_at, vfs_open_description,
        },
        task::files::{
            Fd, FdFlags, FileDesc, FileDescOps, FileStatusFlags, LinuxOpenCompat, OpenAccessMode,
        },
    };

    fn isolated(capacity: usize) -> DevptsInstance {
        DevptsInstance::try_new(Arc::new(FileSystem::new(&DEVPTS_FS_OPS)), capacity).unwrap()
    }

    fn line() -> TtyLineSnapshot {
        TtyLineSnapshot {
            baud: 38400,
            parity: TtyParity::None,
            data_bits: 8,
        }
    }

    fn prepared_binding(instance: &DevptsInstance, uid: Uid, gid: Gid) -> Arc<DevptsBinding> {
        let reservation = instance.core.reserve().unwrap();
        let pair = prepare_pair(reservation.episode.index as u32, line()).unwrap();
        DevptsBinding::prepare(instance, reservation, pair.pair_handle(), uid, gid).unwrap()
    }

    fn root_path(instance: &DevptsInstance) -> PathRef {
        PathRef::new(
            instance.root_mount.clone(),
            instance.root_mount.root().clone(),
        )
    }

    fn commit_ptmx(instance: &DevptsInstance) -> (Fd, Arc<FileDesc>, Arc<DevptsBinding>) {
        let reservation = get_current_task().reserve_fd().unwrap();
        let opened = instance.prepare_ptmx().unwrap();
        let OpenedFile {
            file_ops,
            mode,
            prv,
            description_activation,
        } = opened;
        let prepared = description_activation
            .expect("ptmx must activate one opened description")
            .prepare(
                FileOpenRequest::new(FileOpenAccess::ReadWrite, FileOpStatusFlags::empty(), true),
                observed_file_description_ops(),
            )
            .unwrap();
        let file = File::new_with_mode(root_path(instance), file_ops, mode, prv);
        file.check_status_flags(FileOpStatusFlags::empty()).unwrap();
        let description = FileDesc::new_opened(
            file,
            OpenAccessMode::ReadWrite,
            FileStatusFlags::empty(),
            LinuxOpenCompat::empty(),
            FdFlags::empty(),
            prepared.description_ops,
        );
        prepared.commit.commit(description.clone()).unwrap();
        let binding = instance.core.binding(0).unwrap();
        let fd = reservation.commit(description.clone());
        (fd, description, binding)
    }

    fn unused_ioctl_fd_lookup(_raw_fd: u64) -> Result<IoctlArgFile, SysError> {
        Err(SysError::BadFileDescriptor)
    }

    fn reserve_current_fd() -> Result<crate::task::files::FdReservation, SysError> {
        get_current_task().reserve_fd()
    }

    #[kunit]
    fn mount_callback_reuses_persistent_superblock_and_rejects_data() {
        let fs = system_instance().sb.fs().clone();
        let first = fs.mount(MountSource::Pseudo, MountData::Null).unwrap();
        let second = fs
            .mount(MountSource::Pseudo, MountData::Text(Box::from("")))
            .unwrap();
        assert!(Arc::ptr_eq(&first, &second));
        assert_eq!(first.root_inode(), second.root_inode());
        assert_eq!(
            fs.mount(
                MountSource::Pseudo,
                MountData::Text(Box::from("newinstance")),
            )
            .unwrap_err(),
            SysError::InvalidArgument
        );

        let weak = Arc::downgrade(&first);
        let first_root = Arc::new(Dentry::new("/".to_string(), None, first.root_inode()));
        let second_root = Arc::new(Dentry::new("/".to_string(), None, second.root_inode()));
        let first_view = Arc::new(Mount::new(
            first_root,
            first.clone(),
            MountAttrFlags::empty(),
        ));
        let second_view = Arc::new(Mount::new(
            second_root,
            second.clone(),
            MountAttrFlags::empty(),
        ));
        drop(first_view);
        drop(second_view);
        drop(first);
        drop(second);
        assert!(weak.upgrade().is_some());
    }

    #[kunit]
    fn live_binding_does_not_block_last_view_unmount_and_remount() {
        let tree = MountTree::new();
        tree.mount_root(
            get_filesystem("ramfs").unwrap(),
            MountSource::Pseudo,
            MountAttrFlags::empty(),
        )
        .unwrap();
        let root = tree.root_path().unwrap();
        let target = vfs_mkdir_at(
            &root,
            "devpts-last-view",
            InodePerm::all_rwx(),
            Uid::ROOT,
            Gid::ROOT,
        )
        .unwrap();
        let instance = system_instance();
        let binding = prepared_binding(instance, Uid::ROOT, Gid::ROOT);
        let episode = binding.episode();
        DevptsBinding::publish(&binding);

        let first = tree
            .mount_at(
                instance.sb.fs().clone(),
                MountSource::Pseudo,
                MountAttrFlags::empty(),
                &target,
            )
            .unwrap();
        assert!(!instance.sb.has_alive_inode());
        tree.unmount(&first).unwrap();
        assert!(Arc::ptr_eq(
            &instance.core.binding(episode.index).unwrap(),
            &binding
        ));

        let second = tree
            .mount_at(
                instance.sb.fs().clone(),
                MountSource::Pseudo,
                MountAttrFlags::empty(),
                &target,
            )
            .unwrap();
        assert!(Arc::ptr_eq(second.sb(), &instance.sb));
        tree.unmount(&second).unwrap();
        binding.retire_inner();
    }

    #[kunit]
    fn reservation_capacity_rollback_and_index_reuse() {
        let core = DevptsCore::try_new(2).unwrap();
        let first = core.reserve().unwrap();
        let first_episode = first.episode;
        let second = core.reserve().unwrap();
        assert_eq!(core.used(), 2);
        assert!(matches!(core.reserve(), Err(SysError::NoSpace)));
        drop(first);
        assert_eq!(core.used(), 1);
        let reused = core.reserve().unwrap();
        assert_eq!(reused.episode.index, first_episode.index);
        assert!(reused.episode.generation != first_episode.generation);
        assert!(reused.episode.ino != first_episode.ino);
        drop(reused);
        drop(second);
        assert_eq!(core.used(), 0);
    }

    #[kunit]
    fn ptmx_activation_abort_and_commit_follow_production_transaction() {
        let instance = isolated(1);
        let opened = instance.prepare_ptmx().unwrap();
        assert_eq!(instance.core.used(), 1);
        drop(opened);
        assert_eq!(instance.core.used(), 0);

        let opened = instance.prepare_ptmx().unwrap();
        let OpenedFile {
            file_ops: _,
            mode: _,
            prv: _,
            description_activation,
        } = opened;
        let prepared = description_activation
            .unwrap()
            .prepare(
                FileOpenRequest::new(FileOpenAccess::ReadWrite, FileOpStatusFlags::empty(), true),
                FileDescOps::default(),
            )
            .unwrap();
        drop(prepared);
        assert_eq!(instance.core.used(), 0);

        let (master_fd, master, binding) = commit_ptmx(&instance);
        assert!(Arc::ptr_eq(
            &get_current_task().get_fd(master_fd).unwrap(),
            &master
        ));
        assert!(binding.is_active());
        assert_eq!(binding.pair().live_slave_count(), Some(0));
        get_current_task().close_fd(master_fd).unwrap();
        assert!(!binding.is_active());
        assert_eq!(instance.core.used(), 0);
    }

    #[kunit]
    fn pathname_activation_commits_one_slave_description() {
        let instance = isolated(1);
        let (master_fd, _master, binding) = commit_ptmx(&instance);
        binding.pair().unlock_slave();
        let reservation = get_current_task().reserve_fd().unwrap();
        let opened = vfs_open_description(
            binding.peer_path().unwrap(),
            OpenAccessMode::ReadWrite,
            FileOpStatusFlags::empty(),
            true,
            observed_file_description_ops(),
        )
        .unwrap();
        let (file, description_ops, commit) = opened.into_parts();
        let description = FileDesc::new_opened(
            file,
            OpenAccessMode::ReadWrite,
            FileStatusFlags::empty(),
            LinuxOpenCompat::empty(),
            FdFlags::empty(),
            description_ops,
        );
        commit.unwrap().commit(description.clone()).unwrap();
        assert_eq!(binding.pair().live_slave_count(), Some(1));
        let slave_fd = reservation.commit(description);
        get_current_task().close_fd(slave_fd).unwrap();
        assert_eq!(binding.pair().live_slave_count(), Some(0));
        get_current_task().close_fd(master_fd).unwrap();
    }

    #[kunit]
    fn peer_ioctl_uses_fd_reservation_and_shared_slave_admission() {
        use anemone_abi::fs::linux::open::{O_CLOEXEC, O_NOCTTY, O_NONBLOCK, O_RDWR};

        let instance = isolated(1);
        let (master_fd, master, binding) = commit_ptmx(&instance);
        binding.pair().unlock_slave();
        let lookup = IoctlArgFdLookup::new(unused_ioctl_fd_lookup);
        let installer = IoctlFdInstaller::new(reserve_current_fd);
        let uspace = Arc::new(
            UserSpaceHandle::new(UserSpace::new().unwrap(), root_path(&instance)).unwrap(),
        );
        let ctx = IoctlCtx::new(
            anemone_abi::tty::linux::TIOCGPTPEER,
            u64::from(O_RDWR | O_NOCTTY | O_NONBLOCK | O_CLOEXEC),
            master.ioctl_access(),
            uspace,
            &lookup,
            &installer,
        );
        let peer_fd = Fd::new(master.vfs_file().ioctl(ctx).unwrap() as u32).unwrap();
        let peer = get_current_task().get_fd(peer_fd).unwrap();
        assert!(peer.fd_flags().contains(FdFlags::CLOSE_ON_EXEC));
        assert!(peer.file_flags().contains(FileStatusFlags::NONBLOCK));
        assert_eq!(binding.pair().live_slave_count(), Some(1));
        get_current_task().close_fd(peer_fd).unwrap();
        assert_eq!(binding.pair().live_slave_count(), Some(0));
        get_current_task().close_fd(master_fd).unwrap();
    }

    #[kunit]
    fn unpublished_binding_rolls_back_inode_and_capacity() {
        let instance = isolated(1);
        let binding = prepared_binding(&instance, Uid::new(41), Gid::new(42));
        let episode = binding.episode();
        assert_eq!(instance.core.used(), 1);
        assert!(instance.sb.try_iget(episode.ino).is_some());
        drop(binding);
        assert_eq!(instance.core.used(), 0);
        assert!(instance.sb.try_iget(episode.ino).is_none());
    }

    #[kunit]
    fn binding_metadata_lookup_readdir_and_stale_episode_fail_closed() {
        let instance = isolated(1);
        let old = prepared_binding(&instance, Uid::new(51), Gid::new(52));
        let old_episode = old.episode();
        DevptsBinding::publish(&old);
        let old_path = old.peer_path().unwrap();
        let old_inode = old_path.inode().inode().clone();
        let attr = old_path.inode().get_attr().unwrap();
        assert_eq!(attr.ino, old_episode.ino);
        assert_eq!(attr.uid, Uid::new(51));
        assert_eq!(attr.gid, Gid::new(52));
        assert_eq!(attr.mode.perm(), InodePerm::IRUSR | InodePerm::IWUSR);
        assert_eq!(
            attr.rdev,
            DeviceId::Number(DeviceNumber::new(
                MajorNum::new(DEVPTS_SLAVE_MAJOR),
                MinorNum::new(old_episode.index),
            ))
        );
        let looked_up = instance
            .sb
            .root_inode()
            .lookup(old_episode.index.to_string().as_str())
            .unwrap();
        assert_eq!(looked_up.ino(), old_episode.ino);

        let root_file = PathRef::new(
            instance.root_mount.clone(),
            instance.root_mount.root().clone(),
        )
        .open()
        .unwrap();
        let mut sink = FixedSizeDirSink::<4>::new();
        assert!(matches!(
            root_file.read_dir(&mut sink).unwrap(),
            ReadDirResult::Progressed
        ));
        assert_eq!(sink.entries()[2].name, old_episode.index.to_string());
        assert_eq!(sink.entries()[2].ino, old_episode.ino);

        old.retire_inner();
        let new = prepared_binding(&instance, Uid::new(61), Gid::new(62));
        let new_episode = new.episode();
        assert_eq!(new_episode.index, old_episode.index);
        assert!(new_episode.ino != old_episode.ino);
        DevptsBinding::publish(&new);
        old.retire_inner();
        assert!(Arc::ptr_eq(
            &instance.core.binding(new_episode.index).unwrap(),
            &new
        ));
        assert!(matches!(old_path.inode().open(), Err(SysError::IO)));
        assert_eq!(
            instance
                .sb
                .root_inode()
                .lookup(new_episode.index.to_string().as_str())
                .unwrap()
                .ino(),
            new_episode.ino
        );
        new.retire_inner();
        assert_eq!(instance.core.used(), 0);
        assert_eq!(instance.core.retired_inode_count(), 1);
        drop(old_path);
        drop(looked_up);
        instance.core.retry_retired_inode_eviction();
        assert_eq!(instance.core.retired_inode_count(), 0);
        assert!(
            instance
                .sb
                .cached_inode_snapshot(false)
                .iter()
                .all(|inode| !Arc::ptr_eq(inode, &old_inode))
        );
    }

    #[kunit]
    fn locked_live_pair_rejects_slave_description_prepare() {
        let instance = isolated(1);
        let (master_fd, _master, binding) = commit_ptmx(&instance);
        assert!(matches!(
            binding.pair().prepare_slave_description(),
            Err(SysError::IO)
        ));
        assert_eq!(
            binding.peer_path().unwrap().inode().ino(),
            binding.episode().ino
        );
        get_current_task().close_fd(master_fd).unwrap();
    }

    #[kunit]
    fn raw_kernel_open_rejects_activation_without_slave_participation() {
        let instance = isolated(1);
        let (master_fd, _master, binding) = commit_ptmx(&instance);
        binding.pair().unlock_slave();

        assert!(matches!(
            binding.peer_path().unwrap().open(),
            Err(SysError::NotSupported)
        ));
        assert_eq!(binding.pair().live_slave_count(), Some(0));

        get_current_task().close_fd(master_fd).unwrap();
    }
}
