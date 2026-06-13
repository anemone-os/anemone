use crate::prelude::*;

// Only backing filesystems call this; ramfs/shm/anonymous pages are not part
// of this report.
static RESIDENT_BACKING_FILE_CACHE_PAGES: AtomicUsize = AtomicUsize::new(0);

pub fn backing_file_cache_page_inserted() {
    RESIDENT_BACKING_FILE_CACHE_PAGES.fetch_add(1, Ordering::Relaxed);
}

pub fn backing_file_cache_pages_removed(npages: usize) {
    if npages == 0 {
        return;
    }

    let old = RESIDENT_BACKING_FILE_CACHE_PAGES.fetch_sub(npages, Ordering::Relaxed);
    assert!(old >= npages, "resident file cache page counter underflow");
}

pub fn resident_file_inode_cache_pages() -> usize {
    RESIDENT_BACKING_FILE_CACHE_PAGES.load(Ordering::Relaxed)
}
