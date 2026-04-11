use crate::{device::CpuArchTrait, prelude::*, sched::clone_current_task};

pub fn handle_user_page_fault(info: PageFaultInfo) {
    if let Err(e) = handle_user_page_fault_internal(info) {
        kerrln!(
            "({}) user {} aborted with page fault at address {:?} with pc: {:?}, error type: {:?}, error code: {:?}",
            CpuArch::cur_cpu_id(),
            current_task_id(),
            info.fault_addr(),
            info.fault_pc(),
            info.fault_type(),
            e
        );
        kernel_exit(-1);
    }
}

pub fn handle_user_page_fault_internal(info: PageFaultInfo) -> Result<(), MmError> {
    let task = clone_current_task();
    let uspace = task
        .clone_uspace()
        .expect("user task should have a user space");
    uspace.write().handle_page_fault(&info)?;

    Ok(())
}
