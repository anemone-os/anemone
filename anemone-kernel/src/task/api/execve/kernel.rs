use crate::{
    prelude::*,
    task::{cpu_usage::Privilege, execve::binfmt::dispatch_execve},
};

/// **This function must be run in a process context.**
///
/// `path` is not a global namespace path, but rather a path relative to the
/// current process's filesystem context. **This is important for security and
/// isolation.**
pub fn kernel_execve(
    path: &impl AsRef<str>,
    argv: &[impl AsRef<str>],
    envp: &[impl AsRef<str>],
) -> Result<(), SysError> {
    let usp = UserSpace::new_user()?;
    let mut usp_data = usp.write();
    match dispatch_execve(&mut usp_data, path.as_ref(), argv, envp) {
        Ok(meta) => {
            drop(usp_data);
            let usp = Arc::new(usp);
            unsafe {
                IntrArch::local_intr_disable();
                usp.activate();
                let mut ksp = VirtAddr::new(0);

                let task = get_current_task();

                task.close_cloexec_fds();

                // this must be a user task.
                let exec_fn = path.as_ref().split('/').last().unwrap_or(path.as_ref());
                let name = (String::from("@user/") + exec_fn).into_boxed_str();
                let flags = TaskFlags::NONE;
                task.switch_exec_ctx(name, usp, flags);

                ksp = task.kstack().stack_top();
                task.on_prv_change(Privilege::User);

                // DROP
                drop(task);

                load_context(TaskContext::from_user_fn(meta.entry, meta.sp, ksp));
            }
        },
        Err(SysError::BinFmtUnrecognized) => {
            return Err(SysError::BinFmtUnrecognized);
        },
        Err(e) => {
            kwarningln!("failed to load binary: {e:?}");
            return Err(e);
        },
    }
}
