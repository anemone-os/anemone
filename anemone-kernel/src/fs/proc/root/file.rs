use crate::{fs::proc::root::PROC_ROOT_INO, prelude::*};

const PROC_ROOT_DOT_CURSOR: usize = 0;
const PROC_ROOT_DOTDOT_CURSOR: usize = 1;
const PROC_ROOT_SELF_CURSOR: usize = 2;
const PROC_ROOT_TGID_CURSOR_BASE: usize = 3;

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

    loop {
        match *pos {
            PROC_ROOT_DOT_CURSOR => {
                match push_root_entry(sink, ".", PROC_ROOT_INO, InodeType::Dir)? {
                    SinkResult::Accepted => {
                        pushed_any = true;
                        *pos = PROC_ROOT_DOTDOT_CURSOR;
                    },
                    SinkResult::Stop => {
                        return Ok(if pushed_any {
                            ReadDirResult::Progressed
                        } else {
                            ReadDirResult::Eof
                        });
                    },
                }
            },
            PROC_ROOT_DOTDOT_CURSOR => {
                match push_root_entry(sink, "..", PROC_ROOT_INO, InodeType::Dir)? {
                    SinkResult::Accepted => {
                        pushed_any = true;
                        *pos = PROC_ROOT_SELF_CURSOR;
                    },
                    SinkResult::Stop => {
                        return Ok(if pushed_any {
                            ReadDirResult::Progressed
                        } else {
                            ReadDirResult::Eof
                        });
                    },
                }
            },
            PROC_ROOT_SELF_CURSOR => {
                // `self` is currently still a lookup-time rewrite instead of a
                // real symlink inode, so keep `d_type` aligned with the current
                // behavior and avoid touching binding state in readdir.
                match push_root_entry(sink, "self", Ino::INVALID, InodeType::Dir)? {
                    SinkResult::Accepted => {
                        pushed_any = true;
                        *pos = PROC_ROOT_TGID_CURSOR_BASE;
                    },
                    SinkResult::Stop => {
                        return Ok(if pushed_any {
                            ReadDirResult::Progressed
                        } else {
                            ReadDirResult::Eof
                        });
                    },
                }
            },
            _ => break,
        }
    }

    let from = if *pos == PROC_ROOT_TGID_CURSOR_BASE {
        None
    } else {
        Some(Tid::new((*pos - PROC_ROOT_TGID_CURSOR_BASE) as u32))
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
                    *pos = PROC_ROOT_TGID_CURSOR_BASE
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
    read: |_, _, _| Err(SysError::IsDir),
    write: |_, _, _| Err(SysError::IsDir),
    validate_seek: |_, _| Err(SysError::IsDir),
    read_dir: proc_root_read_dir,
};
