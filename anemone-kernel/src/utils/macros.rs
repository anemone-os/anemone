//! Here lies general macros used throughout Anemone.

#[macro_export]
macro_rules! static_assert {
    ($prediction:expr) => {
        const _: () = assert!($prediction);
    };
    ($prediction:expr, $($msg:literal)?) => {
        const _: () = assert!($prediction, $($msg)?);
    };
}
