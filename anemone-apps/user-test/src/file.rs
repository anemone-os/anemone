use anemone_rs::{os::linux::fs::write, prelude::*};

pub(crate) fn write_all(fd: u32, mut buf: &[u8], path: &str) {
    while !buf.is_empty() {
        let written = write(fd, buf)
            .unwrap_or_else(|errno| panic!("user-test: failed to write {path}: {errno:?}"));
        if written == 0 {
            panic!("user-test: short write while writing {path}");
        }
        buf = &buf[written..];
    }
}
