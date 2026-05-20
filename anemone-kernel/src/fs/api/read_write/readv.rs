//! readv system call.
//!
//! Reference:
//! - https://www.man7.org/linux/man-pages/man2/readv.2.html

use crate::{prelude::{user_access::user_addr, *}, task::files::Fd};

use super::{current_file_and_uspace, load_iovecs, read_iovecs};

#[syscall(SYS_READV)]
fn sys_readv(
	fd: Fd,
	#[validate_with(user_addr)] iov: VirtAddr,
	iovcnt: usize,
) -> Result<u64, SysError> {
	let (file, uspace) = current_file_and_uspace(fd)?;
	let iovecs = load_iovecs(&uspace, iov, iovcnt)?;
	read_iovecs(&file, &uspace, &iovecs, None)
}
