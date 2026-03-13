#![allow(unused)]

use alloc::boxed::Box;
use alloc::sync::{Arc, Weak};
use alloc::vec::Vec;
use core::cell::UnsafeCell;
//需要后续补充完善
//use crate::fs::File;
//use crate::mm::MemorySet;
use crate::mm::addr::*;
use crate::sched::hal::TaskContext;
use crate::sync::spinlock::SpinLock;
use crate::sched::flags::TaskCloneFlags;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskState { Ready, Running, Blocked, Exited }

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct TaskId(pub usize);

pub struct Task {
    pub id: TaskId,
    pub parent: SpinLock<Option<Weak<Task>>>,
    pub children: SpinLock<Vec<Arc<Task>>>,

    pub state: SpinLock<TaskState>,
    pub context: UnsafeCell<TaskContext>, // 内核切换上下文 (ra, sp, s0-s11)
    pub kstack: Box<[u8]>,                // 每个 Task 拥有独立的内核栈

    pub memory_set: Arc<SpinLock<MemorySet>>,
    pub fd_table: Arc<SpinLock<Vec<Option<Arc<dyn File + Send + Sync>>>>>,

    pub exit_code: SpinLock<i32>,
}

unsafe impl Sync for Task {}
unsafe impl Send for Task {}

impl Task {
    pub fn copy_process(
        self: &Arc<Self>,
        new_id: TaskId,
        flags: TaskCloneFlags,
        user_stack: usize,
    ) -> Arc<Self> {
        let child_mm = if flags.contains(TaskCloneFlags::CLONE_VM) {
            // for thread
            Arc::clone(&self.memory_set)
        } else {
            // for process
            let parent_mm = self.memory_set.lock();
            Arc::new(SpinLock::new(parent_mm.clone_space())) //clone_space need to be done
        };

        let child_fds = if flags.contains(TaskCloneFlags::CLONE_FILES) {
            // 线程共享 Arc
            Arc::clone(&self.fd_table)
        } else {
            // 进程拷贝一份当前已打开文件的引用
            Arc::new(SpinLock::new(self.fd_table.lock().clone()))
        };

        let kstack_size = self.kstack.len();
        let kstack = unsafe { Box::<[u8]>::new_uninit_slice(kstack_size).assume_init() };
        let kstack_top = kstack.as_ptr() as usize + kstack_size;

        let child = Arc::new(Task {
            id: new_id,
            parent: SpinLock::new(Some(Arc::downgrade(self))),
            children: SpinLock::new(Vec::new()),
            state: SpinLock::new(TaskState::Ready),
            context: UnsafeCell::new(unsafe { TaskContext::goto_trap_return(kstack_top) }),
            kstack,
            memory_set: child_mm,
            fd_table: child_fds,
            exit_code: SpinLock::new(0),
        });

        let trap_cx = child.get_trap_cx();
        trap_cx.kernel_sp = kstack_top;

        if flags.contains(TaskCloneFlags::CLONE_VM) {
            // 线程，指定新的用户栈指针
            trap_cx.x[2] = user_stack; 
        } else {
            // 进程返回值为 0
            trap_cx.x[10] = 0; 
        }

        self.children.lock().push(Arc::clone(&child));

        child
    }
}

impl Task {

    pub fn fork(self: &Arc<Self>, new_id: TaskId) -> Arc<Self> {
        self.copy_process(new_id, TaskCloneFlags::empty(), 0)
    }

    pub fn clone_thread(self: &Arc<Self>, new_id: TaskId, user_stack: usize) -> Arc<Self> {
        self.copy_process(
            new_id, 
            TaskCloneFlags::CLONE_VM | TaskCloneFlags::CLONE_FILES, 
            user_stack
        )
    }

    pub fn exec(&self, elf_data: &[u8]) {
        let (memory_set, user_sp, entry_point) = MemorySet::from_elf(elf_data);
        *self.memory_set.lock() = memory_set;
        let trap_cx = self.get_trap_cx();
    
        for i in 0..32 { trap_cx.x[i] = 0; }
        trap_cx.x[2] = user_sp; 
        trap_cx.sepc = entry_point;
        
        // 还需要完善
    }
}
