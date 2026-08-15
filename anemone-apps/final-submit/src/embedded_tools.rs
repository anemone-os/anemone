use anemone_rs::{
    abi::fs::linux::open::{O_CREAT, O_TRUNC, O_WRONLY},
    os::linux::fs::{AtFd, close, openat},
    prelude::*,
};

cfg_select! {
    target_arch = "riscv64" => {
        const MKE2FS: &[u8] = include_bytes!("../../user-test/staged/riscv/mke2fs");
    },
    target_arch = "loongarch64" => {
        const MKE2FS: &[u8] = include_bytes!("../../user-test/staged/loongarch/mke2fs");
    }
}

const MKE2FS_PATH: &str = "/bin/mke2fs";

pub(crate) fn install() -> Result<(), Errno> {
    // The frozen preliminary disk is not a complete rootfs. This submission-
    // only bridge supplies the device helper expected by selected LTP cases;
    // remove it together with `final-submit` after the final evaluation.
    // `busybox --install -s /bin` may have published `/bin/mke2fs` as an
    // applet symlink. Remove it before O_TRUNC so opening the destination
    // cannot follow the link and overwrite the bootstrap BusyBox itself.
    crate::busybox::run_busybox(&["busybox", "rm", "-f", MKE2FS_PATH], MKE2FS_PATH);
    let fd = openat(
        AtFd::Cwd,
        Path::new(MKE2FS_PATH),
        O_WRONLY | O_CREAT | O_TRUNC,
        0o755,
    )?;
    crate::file::write_all(fd, MKE2FS, MKE2FS_PATH);
    close(fd)?;

    for link in ["/bin/mkfs.ext3", "/bin/mkfs.ext4"] {
        crate::busybox::run_busybox(&["busybox", "rm", "-f", link], link);
        crate::busybox::run_busybox(&["busybox", "ln", "-s", MKE2FS_PATH, link], link);
    }
    println!("final-submit: installed embedded mke2fs compatibility tool");
    Ok(())
}
