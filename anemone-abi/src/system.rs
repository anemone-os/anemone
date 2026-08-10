pub mod linux {
    #[derive(
        Debug,
        Clone,
        Copy,
        PartialEq,
        Eq,
        Default,
        zerocopy::FromBytes,
        zerocopy::Immutable,
        zerocopy::IntoBytes,
    )]
    #[repr(C)]
    pub struct SysInfo {
        pub uptime: i64,
        pub loads: [u64; 3],
        pub totalram: u64,
        pub freeram: u64,
        pub sharedram: u64,
        pub bufferram: u64,
        pub totalswap: u64,
        pub freeswap: u64,
        pub procs: u16,
        pub pad: u16,
        pub __reserved0: u32,
        pub totalhigh: u64,
        pub freehigh: u64,
        pub mem_unit: u32,
        pub _f: [u8; 20 - 2 * size_of::<u64>() - size_of::<u32>()],
        pub __reserved1: u32,
    }

    const _: () = assert!(size_of::<SysInfo>() == 112);
    const _: () = assert!(core::mem::offset_of!(SysInfo, totalhigh) == 88);
    const _: () = assert!(core::mem::offset_of!(SysInfo, mem_unit) == 104);
}

pub mod native {
    pub mod perf {
        pub const PERF_OBSERVE_QUERY: u64 = 0;
        pub const PERF_OBSERVE_GET_ENABLED: u64 = 1;
        pub const PERF_OBSERVE_SET_ENABLED: u64 = 2;
        pub const PERF_OBSERVE_SNAPSHOT: u64 = 3;

        pub const PERF_CLOCK_MONOTONIC_RAW: u32 = 1;

        pub const PERF_METRIC_COUNTER: u16 = 1;
        pub const PERF_METRIC_HISTOGRAM: u16 = 2;
        /// Completed interval samples represented as `[count, sum_ticks]`.
        pub const PERF_METRIC_ELAPSED: u16 = 3;

        pub const PERF_UNIT_EVENTS: u16 = 1;
        pub const PERF_UNIT_MONOTONIC_TICKS: u16 = 2;

        pub const PERF_HISTOGRAM_BUCKET_COUNT: usize = 65;
        /// Histogram snapshot values are the log2 buckets followed by the
        /// wrapping sum of every recorded sample in the metric's unit.
        pub const PERF_HISTOGRAM_SUM_INDEX: usize = PERF_HISTOGRAM_BUCKET_COUNT;
        pub const PERF_HISTOGRAM_VALUE_COUNT: usize = PERF_HISTOGRAM_BUCKET_COUNT + 1;

        pub const PERF_ELAPSED_SAMPLE_COUNT_INDEX: usize = 0;
        pub const PERF_ELAPSED_SUM_INDEX: usize = 1;
        pub const PERF_ELAPSED_VALUE_COUNT: usize = 2;

        pub const PERF_CATALOG_HEADER_SIZE: usize = 32;
        pub const PERF_CATALOG_CLOCK_KIND_OFFSET: usize = 0;
        pub const PERF_CATALOG_METRIC_COUNT_OFFSET: usize = 4;
        pub const PERF_CATALOG_VALUE_COUNT_OFFSET: usize = 8;
        pub const PERF_CATALOG_HISTOGRAM_BUCKET_COUNT_OFFSET: usize = 12;
        pub const PERF_CATALOG_CLOCK_FREQUENCY_HZ_OFFSET: usize = 16;
        pub const PERF_CATALOG_NAME_BYTES_OFFSET: usize = 24;
        pub const PERF_CATALOG_RESERVED_OFFSET: usize = 28;

        pub const PERF_METRIC_DESCRIPTOR_SIZE: usize = 24;
        pub const PERF_METRIC_ID_OFFSET: usize = 0;
        pub const PERF_METRIC_KIND_OFFSET: usize = 4;
        pub const PERF_METRIC_UNIT_OFFSET: usize = 6;
        pub const PERF_METRIC_VALUE_OFFSET_OFFSET: usize = 8;
        pub const PERF_METRIC_VALUE_COUNT_OFFSET: usize = 12;
        pub const PERF_METRIC_NAME_LEN_OFFSET: usize = 14;
        pub const PERF_METRIC_NAME_OFFSET_OFFSET: usize = 16;
        pub const PERF_METRIC_RESERVED_OFFSET: usize = 20;

        pub const PERF_SNAPSHOT_HEADER_SIZE: usize = 24;
        pub const PERF_SNAPSHOT_BEGIN_TICKS_OFFSET: usize = 0;
        pub const PERF_SNAPSHOT_END_TICKS_OFFSET: usize = 8;
        pub const PERF_SNAPSHOT_VALUE_COUNT_OFFSET: usize = 16;
        pub const PERF_SNAPSHOT_ENABLED_OFFSET: usize = 20;
        pub const PERF_SNAPSHOT_RESERVED_OFFSET: usize = 21;

        const _: () = assert!(PERF_CATALOG_RESERVED_OFFSET + size_of::<u32>() == 32);
        const _: () = assert!(PERF_METRIC_RESERVED_OFFSET + size_of::<u32>() == 24);
        const _: () = assert!(PERF_SNAPSHOT_RESERVED_OFFSET + 3 == 24);
        const _: () = assert!(PERF_HISTOGRAM_SUM_INDEX + 1 == PERF_HISTOGRAM_VALUE_COUNT);
        const _: () = assert!(PERF_ELAPSED_SUM_INDEX + 1 == PERF_ELAPSED_VALUE_COUNT);
    }

    pub mod power {
        /// Dead Cell.
        pub const SHUTDOWN_MAGIC: u64 = 0xdeadce11;
    }
}
