use core::ops::{Deref, DerefMut};

cfg_select! {
    target_arch = "riscv64" => {
        pub const L1_CACHE_SHIFT: usize = 6;
        pub const L1_CACHE_BYTES: usize = 1 << L1_CACHE_SHIFT;

        /// riscv64 has 64-byte cache lines.
        ///
        /// Reference:
        /// - https://elixir.bootlin.com/linux/v6.6.32/source/arch/riscv/include/asm/cache.h#L12
        #[repr(align(64))]
        pub struct CachePadded<T>(T);
    }
    target_arch = "loongarch64" => {
        pub const L1_CACHE_SHIFT: usize = 6;
        pub const L1_CACHE_BYTES: usize = 1 << L1_CACHE_SHIFT;

        /// loongarch64 does not have a fixed cache line size.
        /// we assume it is 64 bytes, which is the most common cache line size.
        ///
        /// Reference:
        /// - https://elixir.bootlin.com/linux/v6.6.32/source/arch/loongarch/include/asm/cache.h#L8
        #[repr(align(64))]
        pub struct CachePadded<T>(T);
    }
    _ => {
        compile_error!("unsupported architecture");
    }
}

impl<T> CachePadded<T> {
    pub const fn new(value: T) -> Self {
        Self(value)
    }
}

impl<T> Deref for CachePadded<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<T> DerefMut for CachePadded<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}
