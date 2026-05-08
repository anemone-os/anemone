use crate::{
    prelude::*,
    task::sig::{
        SigNo, Signal,
        info::{SiCode, SigFault, SigInfoFields},
    },
};

pub fn handle_user_page_fault(info: PageFaultInfo) {
    if let Err(e) = handle_user_page_fault_internal(info) {
        kerrln!(
            "({}) user {} aborted with page fault at address {:?} with pc: {:?}, error type: {:?}, error code: {:?}",
            cur_cpu_id(),
            current_task_id(),
            info.fault_addr(),
            info.fault_pc(),
            info.fault_type(),
            e
        );
        get_current_task()
            .get_thread_group()
            .recv_signal(Signal::new(
                SigNo::SIGSEGV,
                SiCode::Kernel,
                SigInfoFields::Fault(SigFault {
                    addr: info.fault_addr(),
                }),
            ));
    }
}

pub fn handle_user_page_fault_internal(info: PageFaultInfo) -> Result<(), SysError> {
    let task = get_current_task();
    let uspace = task.clone_uspace();
    uspace.handle_page_fault(&info)?;

    Ok(())
}
