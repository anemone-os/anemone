use core::sync::atomic::{AtomicUsize, Ordering};
use crate::sync::spinlock::SpinLock;
use alloc::vec::Vec;
use crate::sched::hal::CpuOpsArch;
use crate::arch::CurCpuOpsArch;
use kernel_macros::percpu;
use crate::prelude::*;
use crate::sched::hal::get_cpu_count;

//每次提供给单个cpu的id数量
const ID_APPLY_SIZE: usize = 128;//需不需要调整


/// 全局分配器，只有一个，按组（128）个分配id给不同cpu，所有cpu共享一个id总池
struct GlobalIdState {
    next_id_start: usize,
}

pub struct GlobalTaskIdAllocator {
    state: SpinLock<GlobalIdState>,
}

impl GlobalTaskIdAllocator {
    pub const fn new() -> Self {
        Self {
            state: SpinLock::new(GlobalIdState { next_id_start: 1 }),
        }
    }

    /// 分配一段id给某个CPU
    fn fetch_applied(&self) -> (usize, usize) {
        let mut guard = self.state.lock();
        let start = guard.next_id_start;
        let end = start + ID_APPLY_SIZE;
        guard.next_id_start = end;
        (start, end)
    }
}

/// CPU本地分配器：负责该CPU核心内部的任务id分配
struct LocalIdAllocator {
    current: usize,
    max: usize,
    free_pool: Vec<usize>,
}

impl LocalIdAllocator {
    const fn new() -> Self {
        Self {
            current: 0,
            max: 0,
            free_pool: Vec::new(),
        }
    }
}

// 全局分配器实例化，要不要lazy？
static GLOBAL_HUB: GlobalTaskIdAllocator = GlobalTaskIdAllocator::new();

// 每个CPU独占的本地分配器，要不要lazy？
#[percpu]
static LOCAL_ID_ALLOC: LocalIdAllocator = LocalIdAllocator::new();



/// 分配id
pub fn allocate_task_id() -> usize {
    LOCAL_ID_ALLOC.with_mut(|alloc| {
        
        // 优先考虑本CPU空闲池
        if let Some(id) = alloc.free_pool.pop() {
            return id;
        }

        // 检查本地分段是否用完
        if alloc.current >= alloc.max {
            // 从GLOBAL_HUB获取一段id
            let (start, end) = GLOBAL_HUB.fetch_applied();
            alloc.current = start;
            alloc.max = end;
        }

        let id = alloc.current;
        alloc.current += 1;
        id
    })
}

pub fn free_task_id(id: usize) {
    LOCAL_ID_ALLOC.with_mut(|alloc| {
        alloc.free_pool.push(id);
    });
}