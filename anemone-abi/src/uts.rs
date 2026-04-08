pub mod linux {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    #[repr(C)]
    pub struct OldUtsName {
        pub sysname: [u8; 65],
        pub nodename: [u8; 65],
        pub release: [u8; 65],
        pub version: [u8; 65],
        pub machine: [u8; 65],
    }

    impl OldUtsName {
        pub const ZEROED: Self = Self {
            sysname: [0; 65],
            nodename: [0; 65],
            release: [0; 65],
            version: [0; 65],
            machine: [0; 65],
        };
    }
}
