pub mod linux {
    use core::mem::{align_of, offset_of, size_of};

    pub type TcFlag = u32;
    pub type Cc = u8;
    pub const NCCS: usize = 19;

    #[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
    #[repr(C)]
    pub struct Termios {
        pub c_iflag: TcFlag,
        pub c_oflag: TcFlag,
        pub c_cflag: TcFlag,
        pub c_lflag: TcFlag,
        pub c_line: Cc,
        pub c_cc: [Cc; NCCS],
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
    #[repr(C)]
    pub struct Winsize {
        pub ws_row: u16,
        pub ws_col: u16,
        pub ws_xpixel: u16,
        pub ws_ypixel: u16,
    }

    const _: () = assert!(size_of::<Termios>() == 36);
    const _: () = assert!(align_of::<Termios>() == 4);
    const _: () = assert!(offset_of!(Termios, c_line) == 16);
    const _: () = assert!(offset_of!(Termios, c_cc) == 17);
    const _: () = assert!(size_of::<Winsize>() == 8);
    const _: () = assert!(align_of::<Winsize>() == 2);
    const _: () = assert!(offset_of!(Winsize, ws_col) == 2);
    const _: () = assert!(offset_of!(Winsize, ws_xpixel) == 4);
    const _: () = assert!(offset_of!(Winsize, ws_ypixel) == 6);

    pub const VINTR: usize = 0;
    pub const VQUIT: usize = 1;
    pub const VERASE: usize = 2;
    pub const VKILL: usize = 3;
    pub const VEOF: usize = 4;
    pub const VTIME: usize = 5;
    pub const VMIN: usize = 6;
    pub const VSWTC: usize = 7;
    pub const VSTART: usize = 8;
    pub const VSTOP: usize = 9;
    pub const VSUSP: usize = 10;
    pub const VEOL: usize = 11;
    pub const VREPRINT: usize = 12;
    pub const VDISCARD: usize = 13;
    pub const VWERASE: usize = 14;
    pub const VLNEXT: usize = 15;
    pub const VEOL2: usize = 16;

    pub const ICRNL: TcFlag = 0x0100;
    pub const OPOST: TcFlag = 0x0001;
    pub const ONLCR: TcFlag = 0x0004;
    pub const ISIG: TcFlag = 0x0001;
    pub const ICANON: TcFlag = 0x0002;
    pub const ECHO: TcFlag = 0x0008;
    pub const ECHOE: TcFlag = 0x0010;
    pub const ECHOK: TcFlag = 0x0020;
    pub const ECHONL: TcFlag = 0x0040;

    pub const CBAUD: TcFlag = 0x0000_100f;
    pub const CSIZE: TcFlag = 0x0030;
    pub const CS5: TcFlag = 0x0000;
    pub const CS6: TcFlag = 0x0010;
    pub const CS7: TcFlag = 0x0020;
    pub const CS8: TcFlag = 0x0030;
    pub const CREAD: TcFlag = 0x0080;
    pub const PARENB: TcFlag = 0x0100;
    pub const PARODD: TcFlag = 0x0200;
    pub const CLOCAL: TcFlag = 0x0800;

    pub const B0: TcFlag = 0x0000;
    pub const B50: TcFlag = 0x0001;
    pub const B75: TcFlag = 0x0002;
    pub const B110: TcFlag = 0x0003;
    pub const B134: TcFlag = 0x0004;
    pub const B150: TcFlag = 0x0005;
    pub const B200: TcFlag = 0x0006;
    pub const B300: TcFlag = 0x0007;
    pub const B600: TcFlag = 0x0008;
    pub const B1200: TcFlag = 0x0009;
    pub const B1800: TcFlag = 0x000a;
    pub const B2400: TcFlag = 0x000b;
    pub const B4800: TcFlag = 0x000c;
    pub const B9600: TcFlag = 0x000d;
    pub const B19200: TcFlag = 0x000e;
    pub const B38400: TcFlag = 0x000f;
    pub const B57600: TcFlag = 0x1001;
    pub const B115200: TcFlag = 0x1002;
    pub const B230400: TcFlag = 0x1003;
    pub const B460800: TcFlag = 0x1004;
    pub const B500000: TcFlag = 0x1005;
    pub const B576000: TcFlag = 0x1006;
    pub const B921600: TcFlag = 0x1007;
    pub const B1000000: TcFlag = 0x1008;
    pub const B1152000: TcFlag = 0x1009;
    pub const B1500000: TcFlag = 0x100a;
    pub const B2000000: TcFlag = 0x100b;
    pub const B2500000: TcFlag = 0x100c;
    pub const B3000000: TcFlag = 0x100d;
    pub const B3500000: TcFlag = 0x100e;
    pub const B4000000: TcFlag = 0x100f;

    pub const TCGETS: u32 = 0x5401;
    pub const TCSETS: u32 = 0x5402;
    pub const TCSETSW: u32 = 0x5403;
    pub const TCSETSF: u32 = 0x5404;
    pub const TIOCSCTTY: u32 = 0x540e;
    pub const TIOCGPGRP: u32 = 0x540f;
    pub const TIOCSPGRP: u32 = 0x5410;
    pub const TIOCGWINSZ: u32 = 0x5413;
    pub const TIOCSWINSZ: u32 = 0x5414;
    pub const TIOCNOTTY: u32 = 0x5422;
    pub const TIOCGSID: u32 = 0x5429;
}
