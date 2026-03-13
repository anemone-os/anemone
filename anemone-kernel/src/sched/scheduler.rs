#![allow(unused)]

use alloc::collections::VecDeque;
use alloc::sync::Arc;
use core::cell::UnsafeCell;

use crate::sched::hal::{ContextSwitchArch, CpuOpsArch, TaskContext};
use crate::sched::id::allocate_task_id;
use crate::sched::task::{Task, TaskId, TaskState};
use crate::sync::spinlock::SpinLock;


use crate::arch::{CurContextSwitchArch, CurCpuOpsArch};

static SCHEDULER: SpinLock<Option<Scheduler<CurContextSwitchArch, CurCpuOpsArch>>> = SpinLock::new(None);

pub fn init_scheduler() {
    let mut scheduler_guard = SCHEDULER.lock();
    if scheduler_guard.is_none() {
        *scheduler_guard = Some(Scheduler::new(CurContextSwitchArch, CurCpuOpsArch));
    }
}

pub struct Scheduler<CS: ContextSwitchArch, CPU: CpuOpsArch> {
 
    ready_queue: SpinLock<VecDeque<Arc<Task>>>,

    current_task: UnsafeCell<Option<Arc<Task>>>,
    _context_switcher: CS,
    _cpu_ops: CPU,
}

impl<CS: ContextSwitchArch, CPU: CpuOpsArch> Scheduler<CS, CPU> {

    pub fn new(context_switcher: CS, cpu_ops: CPU) -> Self {
        Scheduler {
            ready_queue: SpinLock::new(VecDeque::new()),
            current_task: UnsafeCell::new(None),
            _context_switcher: context_switcher,
            _cpu_ops: cpu_ops,
        }
    }


    fn get_next_task_id(&self) -> TaskId {
        TaskId::new(allocate_task_id())
    }


    pub unsafe fn add_task(&self, entry_point: usize, stack_size: usize) -> Arc<Task> {
        let id = self.get_next_task_id();
        let task = Task::new(id, entry_point, stack_size);
        self.ready_queue.lock().push_back(Arc::clone(&task));
        task
    }


    pub fn current_task(&self) -> Option<Arc<Task>> {
        unsafe { (*self.current_task.get()).clone() }
    }


    pub unsafe fn schedule(&self) {
        let mut ready_queue = self.ready_queue.lock();

        if ready_queue.is_empty() {
            return;
        }

        let current_task_option = unsafe { (*self.current_task.get()).take() };

        if let Some(current_task) = current_task_option {
            if current_task.get_state() == TaskState::Running {
                current_task.set_state(TaskState::Ready);
                ready_queue.push_back(current_task);
            }
        }


        let next_task = ready_queue.pop_front().expect("Ready queue should not be empty");
        next_task.set_state(TaskState::Running);

        unsafe {
            *self.current_task.get() = Some(Arc::clone(&next_task));
        }
 
        let current_context_ptr = if let Some(ref task) = current_task_option {
            task.get_context_mut() as *mut TaskContext
        } else {
            &mut TaskContext::new_empty() as *mut TaskContext
        };

        let next_context_ptr = next_task.get_context() as *const TaskContext;

        unsafe {
            self._context_switcher.switch(&mut *current_context_ptr, &*next_context_ptr);
        }
    }

    pub unsafe fn yield_current_task(&self) {
        unsafe {
            self.schedule();
        }
    }

    pub unsafe fn start_scheduler(&self) -> ! {
        assert!(!self.ready_queue.lock().is_empty(), "Scheduler started with no tasks!");

        let next_task = self.ready_queue.lock().pop_front().expect("Ready queue should not be empty");
        next_task.set_state(TaskState::Running);
        unsafe {
            *self.current_task.get() = Some(Arc::clone(&next_task));
        }

        let next_context_ptr = next_task.get_context() as *const TaskContext;

        let mut dummy_context = TaskContext::new_empty();

        unsafe {
            self._context_switcher.switch(&mut dummy_context, &*next_context_ptr);
        }
        unreachable!();
    }
}

pub fn get_scheduler() -> &'static Scheduler<CurContextSwitchArch, CurCpuOpsArch> {
    SCHEDULER.lock().as_ref().expect("Scheduler not initialized!")
}

pub unsafe fn create_task(entry_point: usize, stack_size: usize) -> Arc<Task> {
    get_scheduler().add_task(entry_point, stack_size)
}

pub fn yield_task() {
    let was_enabled = unsafe { CurCpuOpsArch::disable_interrupts() };
    unsafe { get_scheduler().yield_current_task() };
    unsafe { CurCpuOpsArch::restore_interrupts(was_enabled) };
}

pub unsafe fn start_scheduler() -> ! {
    get_scheduler().start_scheduler()
}