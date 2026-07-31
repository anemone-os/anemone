mod fanotify_init;
mod fanotify_mark;

pub use fanotify_init::sys_fanotify_init;
pub use fanotify_mark::sys_fanotify_mark;
