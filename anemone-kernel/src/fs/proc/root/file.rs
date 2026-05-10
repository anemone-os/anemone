// /// Best effort consistency.
// fn proc_root_iterate(file: &File, ctx: &mut DirContext) -> Result<DirEntry,
// SysError> {     // in iteration, only leader task is visible. non-leader
// threads can be     // looked up, but not iterated.
//
//     todo!()
// }
//
// static PROC_ROOT_FILE_OPS: FileOps = FileOps {
//     read: |_, _| Err(SysError::IsDir),
//     write: |_, _| Err(SysError::IsDir),
//     seek: |_, _| Err(SysError::IsDir),
//     iterate: proc_root_iterate,
// };
