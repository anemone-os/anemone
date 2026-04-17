use crate::{prelude::*, task::execve::binfmt::dispatch_execve};

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
                with_current_task(|task| {
                    task.close_cloexec_fds();
                    let info = TaskExecInfo {
                        cmdline: argv
                            .iter()
                            .map(|s| s.as_ref())
                            .collect::<Vec<_>>()
                            .join(" ")
                            .into(),
                        flags: TaskFlags::NONE,
                        uspace: Some(usp),
                    };
                    unsafe {
                        task.set_exec_info(info);
                    }
                    ksp = task.kstack().stack_top();
                    task.on_prv_change(Privilege::User);
                });
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

    unreachable!();
}
