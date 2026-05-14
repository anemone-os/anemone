use crate::{
    fs::{
        iomux::PollEvent,
        proc::{
            superblock::alloc_ino,
            tgid::{SubInoRecord, TGID_ENTRIES, tgid_inode_private, validate_tgid_inode},
        },
    },
    prelude::*,
};

fn tgid_read_dir(
    file: &File,
    pos: &mut usize,
    sink: &mut dyn DirSink,
) -> Result<ReadDirResult, SysError> {
    let _binding = validate_tgid_inode(file.inode())?;

    let old_pos = *pos;
    if old_pos >= TGID_ENTRIES.len() {
        return Ok(ReadDirResult::Eof);
    }

    let prv = tgid_inode_private(file.inode());
    let mut sub_ino = prv.sub_ino.lock();

    for &entry in &TGID_ENTRIES[old_pos..] {
        let ino = if let Some(SubInoRecord { ino, .. }) = sub_ino.get(entry.name) {
            *ino
        } else {
            // allocate an ino, but not instantiate the inode.
            let ino = alloc_ino();
            sub_ino.insert(
                entry.name,
                SubInoRecord {
                    ino,
                    instantiated: false,
                },
            );
            ino
        };
        match sink.push(DirEntry {
            name: entry.name.to_string(),
            ino,
            ty: entry.mode.ty(),
        })? {
            SinkResult::Accepted => *pos += 1,
            SinkResult::Stop => {
                if *pos == old_pos {
                    return Ok(ReadDirResult::Eof);
                }
                break;
            },
        }
    }

    Ok(ReadDirResult::Progressed)
}

pub static TGID_FILE_OPS: FileOps = FileOps {
    read: |_, _, _| Err(SysError::IsDir),
    write: |_, _, _| Err(SysError::IsDir),
    validate_seek: |_, _| Err(SysError::IsDir),
    read_dir: tgid_read_dir,
    poll: |_, _| Ok(PollEvent::READABLE),
};
