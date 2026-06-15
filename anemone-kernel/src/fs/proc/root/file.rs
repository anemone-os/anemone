use crate::{
    fs::{
        iomux::PollEvent,
        proc::pde::{proc_root_dir_entries, read_pde_root_entries},
    },
    prelude::*,
};

fn proc_root_tgid_cursor_base() -> usize {
    2 + proc_root_dir_entries().len()
}

fn push_root_entry(
    sink: &mut dyn DirSink,
    name: &str,
    ino: Ino,
    ty: InodeType,
) -> Result<SinkResult, SysError> {
    sink.push(DirEntry {
        name: name.to_string(),
        ino,
        ty,
    })
}

/// Best effort consistency.
fn proc_root_read_dir(
    _file: &File,
    pos: &mut usize,
    sink: &mut dyn DirSink,
) -> Result<ReadDirResult, SysError> {
    let mut pushed_any = false;

    let tgid_cursor_base = proc_root_tgid_cursor_base();

    if *pos < tgid_cursor_base {
        match read_pde_root_entries(pos, sink)? {
            ReadDirResult::Progressed => pushed_any = true,
            ReadDirResult::Eof => {},
        }

        if *pos < tgid_cursor_base {
            return Ok(if pushed_any {
                ReadDirResult::Progressed
            } else {
                ReadDirResult::Eof
            });
        }
    }

    let from = if *pos == tgid_cursor_base {
        None
    } else {
        Some(Tid::new((*pos - tgid_cursor_base) as u32))
    };

    let mut stop = false;
    let mut err = None;

    // For `<tgid>` dirents we intentionally report `d_ino = 0` until a real
    // binding exists, so root readdir does not need to consult binding state
    // while topology iteration is holding the topology lock.
    for_each_thread_group_from(
        |tg| {
            if stop || err.is_some() {
                return;
            }

            let tgid = tg.tgid();
            match push_root_entry(sink, &tgid.get().to_string(), Ino::INVALID, InodeType::Dir) {
                Ok(SinkResult::Accepted) => {
                    pushed_any = true;
                    *pos = tgid_cursor_base
                        .checked_add(tgid.get() as usize)
                        .and_then(|value| value.checked_add(1))
                        .expect("proc root readdir cursor overflow");
                },
                Ok(SinkResult::Stop) => {
                    stop = true;
                },
                Err(e) => {
                    err = Some(e);
                },
            }
        },
        from,
    );

    if let Some(e) = err {
        return Err(e);
    }

    Ok(if pushed_any {
        ReadDirResult::Progressed
    } else {
        ReadDirResult::Eof
    })
}

pub static PROC_ROOT_FILE_OPS: FileOps = FileOps {
    read: |_, _, _, _| Err(SysError::IsDir),
    write: |_, _, _, _| Err(SysError::IsDir),
    read_at: |_, _, _, _| Err(SysError::IsDir),
    write_at: |_, _, _, _| Err(SysError::IsDir),
    check_status_flags: accept_file_op_status_flags,
    seek: seek_dir_rewind,
    read_dir: proc_root_read_dir,
    poll: |_, req| Ok(req.ready_or_unsupported(PollEvent::READABLE & req.interests())),
    fcntl: None,
    ioctl: |_, _| Err(SysError::UnsupportedIoctl),
};
