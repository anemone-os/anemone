//! Here lies general macros used throughout Anemone.

/// A compile-time assertion macro. It can be used to check for conditions that
/// must hold at compile time, such as type sizes, trait implementations, or
/// other invariants. If the condition is not met, a compile-time error will be
/// generated with an optional message.
///
/// Used at top level scope.
#[macro_export]
macro_rules! static_assert {
    ($prediction:expr) => {
        const _: () = assert!($prediction);
    };
    ($prediction:expr, $($msg:literal)?) => {
        const _: () = assert!($prediction, $($msg)?);
    };
}

/// A compile-time assertion macro that can be used in function bodies.
///
/// Since the prediction is a const expression, the assertion can be evaluated
/// at compile time, thus making it optimized out and not incurring any runtime
/// overhead.
///
/// If the condition is not met, a compile-time error will be generated with an
/// optional message.
#[macro_export]
macro_rules! const_assert {
    ($prediction:expr) => {
        const {
            assert!($prediction);
        }
    };
    ($prediction:expr, $($msg:literal)?) => {
        const {
            assert!($prediction, $($msg)?);
        }
    };
}
