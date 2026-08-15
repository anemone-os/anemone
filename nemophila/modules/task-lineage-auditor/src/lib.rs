#![no_std]
#![feature(alloc_error_handler)]

extern crate alloc;

use alloc::{collections::BTreeMap, format, rc::Rc};
use core::{alloc::Layout, cell::RefCell, panic::PanicInfo};
use nemophila_sdk::{
    LoadContext, Module,
    services::logging::LogLevel,
    weave::task::{
        RegistrationError,
        thread_exit::{ThreadExitEvent, ThreadExitReason},
    },
};

#[global_allocator]
static ALLOCATOR: dlmalloc::GlobalDlmalloc = dlmalloc::GlobalDlmalloc;

struct TaskLineageAuditor;

impl Module for TaskLineageAuditor {
    type Error = RegistrationError;

    fn load(context: &mut LoadContext<'_>) -> Result<(), Self::Error> {
        let audit = Rc::new(RefCell::new(LineageAudit::default()));
        let clone_audit = audit.clone();
        context
            .weave()
            .task()
            .clone_observer()
            .register(move |event, callback| {
                let message = clone_audit
                    .borrow_mut()
                    .observe_clone(event.creator_tid, event.child_tid);
                callback.logging().write(LogLevel::Notice, &message);
            })?;

        context
            .weave()
            .task()
            .thread_exit()
            .register(move |event, callback| {
                let message = audit.borrow_mut().observe_exit(event);
                callback.logging().write(LogLevel::Notice, &message);
            })?;

        nemophila_sdk::kprintln!(
            context.logging(),
            "task-lineage auditor registered clone and thread-exit observers",
        );
        Ok(())
    }
}

/// Instance-local diagnostic projection. It never decides whether a task is
/// live or exited. Entries are inserted and removed only from the two typed
/// observations serialized by the owning Nemophila instance; missing history
/// therefore remains an explicit `matched=false` result rather than a kernel
/// lifecycle assertion.
#[derive(Default)]
struct LineageAudit {
    creator_by_child: BTreeMap<u32, u32>,
}

impl LineageAudit {
    fn observe_clone(&mut self, creator_tid: u32, child_tid: u32) -> alloc::string::String {
        let replaced = self
            .creator_by_child
            .insert(child_tid, creator_tid)
            .is_some();
        format!(
            "task-lineage clone creator={creator_tid} child={child_tid} replaced={replaced} tracked={}",
            self.creator_by_child.len()
        )
    }

    fn observe_exit(&mut self, event: ThreadExitEvent) -> alloc::string::String {
        let creator = self.creator_by_child.remove(&event.tid);
        let (kind, value) = match event.reason {
            ThreadExitReason::Exited(status) => ("exited", u32::from(status)),
            ThreadExitReason::Signaled(signal) => ("signaled", signal),
        };
        match creator {
            Some(creator) => format!(
                "task-lineage exit tid={} kind={kind} value={value} matched=true creator={creator} tracked={}",
                event.tid,
                self.creator_by_child.len()
            ),
            None => format!(
                "task-lineage exit tid={} kind={kind} value={value} matched=false tracked={}",
                event.tid,
                self.creator_by_child.len()
            ),
        }
    }
}

nemophila_sdk::export_module!(TaskLineageAuditor, task_lifecycle);

#[panic_handler]
fn panic(_info: &PanicInfo<'_>) -> ! {
    core::arch::wasm32::unreachable()
}

#[alloc_error_handler]
fn allocation_error(_layout: Layout) -> ! {
    core::arch::wasm32::unreachable()
}
