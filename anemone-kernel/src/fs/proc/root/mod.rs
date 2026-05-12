mod file;
mod inode;

use crate::prelude::*;

pub const PROC_ROOT_INO: Ino = Ino::new(1);

// TODO: something like linux's `proc_dir_entry` as a infrastructure for
// developers to easily add new files/directories to procfs.
