mod init;
mod mark;

pub use init::sys_fanotify_init;
pub use mark::sys_fanotify_mark;
