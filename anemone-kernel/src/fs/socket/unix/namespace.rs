//! Filesystem pathname creation and exact live-binding identity registry.

use crate::{
    fs::api::creation::{KernelCreationPolicy, kernel_make_node_at},
    prelude::*,
};

use super::endpoint::UnixEndpointCore;

#[derive(Debug)]
struct BindingRecord {
    inode: InodeRef,
    generation: u64,
    endpoint: Weak<UnixEndpointCore>,
}

#[derive(Debug)]
struct BindingRegistry {
    next_generation: u64,
    /// The inode number is only a bucket selector. Every lookup and removal
    /// below compares the complete resident `InodeRef` identity.
    buckets: BTreeMap<Ino, Vec<BindingRecord>>,
}

impl BindingRegistry {
    const fn new() -> Self {
        Self {
            next_generation: 1,
            buckets: BTreeMap::new(),
        }
    }

    fn insert(&mut self, inode: InodeRef, endpoint: &Arc<UnixEndpointCore>) -> BindingRegistration {
        let bucket = self.buckets.entry(inode.ino()).or_default();
        assert!(
            bucket.iter().all(|record| record.inode != inode),
            "Unix binding registry published one inode identity twice"
        );
        let generation = self.next_generation;
        self.next_generation = self
            .next_generation
            .checked_add(1)
            .expect("Unix binding generation exhausted");
        bucket.push(BindingRecord {
            inode: inode.clone(),
            generation,
            endpoint: Arc::downgrade(endpoint),
        });
        BindingRegistration { inode, generation }
    }

    fn remove(&mut self, registration: &BindingRegistration) -> bool {
        let Some(bucket) = self.buckets.get_mut(&registration.inode.ino()) else {
            return false;
        };
        let Some(index) = bucket.iter().position(|record| {
            record.generation == registration.generation && record.inode == registration.inode
        }) else {
            return false;
        };
        bucket.swap_remove(index);
        if bucket.is_empty() {
            self.buckets.remove(&registration.inode.ino());
        }
        true
    }

    fn lookup(&self, inode: &InodeRef) -> Option<LiveBinding> {
        self.buckets.get(&inode.ino())?.iter().find_map(|record| {
            (record.inode == *inode).then(|| LiveBinding {
                inode: record.inode.clone(),
                generation: record.generation,
                endpoint: record.endpoint.clone(),
            })
        })
    }
}

static BINDINGS: Lazy<SpinLock<BindingRegistry>> =
    Lazy::new(|| SpinLock::new(BindingRegistry::new()));

/// Exact removal authority retained by the endpoint that published a binding.
/// Pathname lookup, rename, and hard-link aliases never participate in cleanup.
#[derive(Debug)]
pub(super) struct BindingRegistration {
    inode: InodeRef,
    generation: u64,
}

#[derive(Debug)]
pub(super) struct LiveBinding {
    inode: InodeRef,
    generation: u64,
    endpoint: Weak<UnixEndpointCore>,
}

impl LiveBinding {
    pub(super) fn endpoint(&self) -> Option<Arc<UnixEndpointCore>> {
        self.endpoint.upgrade()
    }

    pub(super) fn matches(&self, inode: &InodeRef, generation: u64) -> bool {
        self.inode == *inode && self.generation == generation
    }

    pub(super) fn matches_registration(&self, registration: &BindingRegistration) -> bool {
        self.matches(&registration.inode, registration.generation)
    }
}

pub(super) fn publish_binding(
    inode: InodeRef,
    endpoint: &Arc<UnixEndpointCore>,
) -> BindingRegistration {
    BINDINGS.lock().insert(inode, endpoint)
}

pub(super) fn withdraw_binding(registration: BindingRegistration) {
    assert!(
        BINDINGS.lock().remove(&registration),
        "Unix binding cleanup lost its exact registration"
    );
}

/// This is the only namespace-to-Unix handoff consumed by connection admission.
pub(super) fn lookup_binding(inode: &InodeRef) -> Option<LiveBinding> {
    BINDINGS.lock().lookup(inode)
}

/// Resolve one connect attempt through the current task namespace and DAC.
/// The returned capability carries no pathname or permission authority and is
/// valid only for the admission revalidation performed by that attempt.
pub(super) fn resolve_live_binding(pathname: &str) -> Result<LiveBinding, SysError> {
    let checker = FsPermChecker::for_current_fs();
    let path = get_current_task().lookup_path_with_checker(
        Path::new(pathname),
        ResolveFlags::empty(),
        &checker,
    )?;
    checker.check_path(&path, FsAccess::WRITE)?;
    if path.inode().ty() != InodeType::Socket {
        return Err(SysError::ConnectionRefused);
    }
    lookup_binding(path.inode()).ok_or(SysError::ConnectionRefused)
}

pub(super) fn create_socket_pathname(pathname: &str) -> Result<InodeRef, SysError> {
    let policy = KernelCreationPolicy::for_current();
    let checker = policy.checker();
    let task = get_current_task();
    let path = Path::new(pathname);
    let (parent, name) =
        match task.lookup_parent_path_with_checker(path, ResolveFlags::empty(), checker) {
            Ok(parent_and_name) => parent_and_name,
            // The current namei creation boundary reports an existing final
            // component through this branch. AF_UNIX maps that case to
            // EADDRINUSE rather than exposing VFS EEXIST.
            Err(SysError::InvalidArgument) => {
                match task.lookup_path_with_checker(path, ResolveFlags::empty(), checker) {
                    Ok(_) => return Err(SysError::AddressInUse),
                    Err(error) => return Err(error),
                }
            },
            Err(error) => return Err(error),
        };

    kernel_make_node_at(
        &policy,
        &parent,
        &name,
        InodeMode::new(InodeType::Socket, InodePerm::all_rwx()),
        DeviceId::None,
    )
    .map(|path| path.inode().clone())
}

#[cfg(feature = "kunit")]
mod kunits {
    use super::*;
    use crate::fs::socket::{UNIX_STREAM_SOCKET_OPS, prepare_socket};

    fn socket_inode() -> (File, InodeRef) {
        let (file, creation) = prepare_socket(&UNIX_STREAM_SOCKET_OPS).unwrap();
        creation.commit();
        let inode = file.inode().clone();
        (file, inode)
    }

    #[kunit]
    fn exact_identity_lookup_and_generation_safe_removal_fail_closed() {
        let endpoint = UnixEndpointCore::new_unconnected();
        let (_first_file, first) = socket_inode();
        let (_second_file, second) = socket_inode();
        let mut registry = BindingRegistry::new();

        let registration = registry.insert(first.clone(), &endpoint);
        let live = registry.lookup(&first).unwrap();
        assert!(live.matches(&first, registration.generation));
        assert!(Arc::ptr_eq(&live.endpoint().unwrap(), &endpoint));

        let stale = BindingRegistration {
            inode: first.clone(),
            generation: registration.generation + 1,
        };
        assert!(!registry.remove(&stale));
        assert!(registry.lookup(&first).is_some());
        assert!(registry.remove(&registration));
        assert!(registry.lookup(&first).is_none());

        // Model an ino-bucket collision/reload directly: bucket selection can
        // lead to a record, but full resident identity still rejects the hit.
        registry.buckets.insert(
            second.ino(),
            vec![BindingRecord {
                inode: first,
                generation: 1,
                endpoint: Arc::downgrade(&endpoint),
            }],
        );
        assert!(registry.lookup(&second).is_none());
    }
}
