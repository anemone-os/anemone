#[macro_export]
macro_rules! static_assert {
    ($prediction:expr $(, $msg:literal)?) => {
        const _: () = assert!($prediction $($msg)?);
    };
}

#[macro_export]
macro_rules! align_down {
    ($addr:expr, $align:expr) => {
        ($addr / $align) * $align
    };
}

#[macro_export]
macro_rules! align_up {
    ($addr:expr, $align:expr) => {
        $crate::align_down!($addr + $align - 1, $align)
    };
}
