use crate::prelude::*;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MountData {
    Null,
    Text(Box<str>),
}

impl MountData {
    pub fn is_empty(&self) -> bool {
        match self {
            Self::Null => true,
            Self::Text(data) => data.is_empty(),
        }
    }

    pub fn has_loop_option(&self) -> bool {
        match self {
            Self::Null => false,
            Self::Text(data) => data
                .split(',')
                .map(str::trim)
                .any(|option| option == "loop" || option.starts_with("loop=")),
        }
    }

    pub fn reject_nonempty_for(&self, fs_name: &str) -> Result<(), SysError> {
        if self.is_empty() {
            return Ok(());
        }

        knoticeln!(
            "mount: filesystem {} rejects non-empty legacy data: empty=false contains_loop={}",
            fs_name,
            self.has_loop_option()
        );
        Err(SysError::InvalidArgument)
    }
}

#[cfg(feature = "kunit")]
mod kunits {
    use super::*;

    #[kunit]
    fn test_mount_data_loop_option_detection_trims_options() {
        assert!(MountData::Text(Box::from("rw, loop")).has_loop_option());
        assert!(MountData::Text(Box::from("loop=/tmp/disk.img")).has_loop_option());
        assert!(!MountData::Text(Box::from("rw")).has_loop_option());
        assert!(!MountData::Null.has_loop_option());
    }

    #[kunit]
    fn test_mount_data_reject_nonempty_for_backend() {
        assert!(MountData::Null.reject_nonempty_for("kunit").is_ok());
        assert_eq!(
            MountData::Text(Box::from("size=64m"))
                .reject_nonempty_for("kunit")
                .unwrap_err(),
            SysError::InvalidArgument
        );
    }
}
