//! This module contains utilities for alignment and related operations.

/// Alignment math helpers.
mod math {
    #[macro_export]
    /// Aligns `addr` upwards to the next multiple of `align`.
    macro_rules! align_up {
        ($addr:expr, $align:expr) => {{
            let addr = $addr as usize;
            let align = $align as usize;
            ((addr + align - 1) / align) * align
        }};
    }

    #[macro_export]
    /// Aligns `addr` downwards to the previous multiple of `align`.
    macro_rules! align_down {
        ($addr:expr, $align:expr) => {{
            let addr = $addr as usize;
            let align = $align as usize;
            (addr / align) * align
        }};
    }

    #[macro_export]
    /// Aligns `addr` upwards to the next power-of-two `align`.
    macro_rules! align_up_power_of_2 {
        ($addr:expr, $align:expr) => {{
            let addr = $addr as usize;
            let align = $align as usize;
            debug_assert!(align.is_power_of_two());
            (addr + align - 1) & !(align - 1)
        }};
    }

    #[macro_export]
    /// Aligns `addr` downwards to the previous power-of-two `align`.
    macro_rules! align_down_power_of_2 {
        ($addr:expr, $align:expr) => {{
            let addr = $addr as usize;
            let align = $align as usize;
            debug_assert!(align.is_power_of_two());
            addr & !(align - 1)
        }};
    }
}

/// Alignment helpers for byte buffers and marker types.
mod bytes {
    use core::ops::{Deref, DerefMut};

    /// Includes bytes with a requested alignment marker.
    #[macro_export]
    macro_rules! include_bytes_aligned_as {
        ($marker:ty, $path:literal) => {{
            use crate::utils::align::*;
            static ALIGNED: &AlignedBytes<$marker, [u8]> = &AlignedBytes::new(*include_bytes!($path));
            &ALIGNED.bytes
        }};
    }

    /// Wraps bytes with an alignment marker.
    ///
    /// This enforces the alignment of the *outer* struct only. If `Bytes`
    /// is a multi-field struct, its fields may still have smaller alignment.
    #[derive(Debug)]
    #[repr(C)]
    pub struct AlignedBytes<Marker: PhantomAligned, Bytes: ?Sized> {
        _align: Marker,
        pub bytes: Bytes,
    }

    impl<Marker: PhantomAligned, Bytes> AlignedBytes<Marker, Bytes> {
        /// Creates an aligned wrapper around `bytes`.
        pub const fn new(bytes: Bytes) -> Self {
            Self {
                _align: Marker::NEW,
                bytes,
            }
        }
    }

    impl<Marker: PhantomAligned, Bytes: Copy + Clone> Clone for AlignedBytes<Marker, Bytes> {
        fn clone(&self) -> Self {
            Self {
                _align: self._align,
                bytes: self.bytes,
            }
        }
    }

    impl<Marker: PhantomAligned, Bytes: Copy + Clone> Copy for AlignedBytes<Marker, Bytes> {}

    impl<Marker: PhantomAligned, const N: usize> AlignedBytes<Marker, [u8; N]> {
        /// A zero-initialized aligned byte array.
        pub const ZEROED: Self = Self {
            _align: Marker::NEW,
            bytes: [0; N],
        };
    }

    impl<Marker: PhantomAligned, const N: usize> Deref for AlignedBytes<Marker, [u8; N]> {
        type Target = [u8; N];

        fn deref(&self) -> &Self::Target {
            &self.bytes
        }
    }

    impl<Marker: PhantomAligned, const N: usize> DerefMut for AlignedBytes<Marker, [u8; N]> {
        fn deref_mut(&mut self) -> &mut Self::Target {
            &mut self.bytes
        }
    }

    /// Marker trait that sets the alignment of the containing type.
    ///
    /// This does not guarantee that each field inside the containing struct
    /// meets the same alignment when the inner type has multiple fields.
    pub unsafe trait PhantomAligned: Copy + Clone + Sized {
        /// A zero-sized marker value for this alignment.
        const NEW: Self;
    }

    macro_rules! def_phantom_aligned {
        ($align:literal) => {
            paste::paste! {
                #[derive(Debug, Clone, Copy)]
                #[repr(align($align))]
                struct [<AlignMarker $align>];

                #[derive(Debug, Clone, Copy)]
                #[repr(transparent)]
                pub struct [<PhantomAligned $align>]([[<AlignMarker $align>]; 0]);

                unsafe impl PhantomAligned for [<PhantomAligned $align>] {
                    const NEW: Self = Self([]);
                }
            }
        };
    }

    def_phantom_aligned!(4);
    def_phantom_aligned!(8);
    def_phantom_aligned!(16);
    def_phantom_aligned!(32);
    def_phantom_aligned!(64);
    def_phantom_aligned!(4096);
    def_phantom_aligned!(16384);
}
/// Re-export alignment helpers and marker types.
pub use bytes::*;
