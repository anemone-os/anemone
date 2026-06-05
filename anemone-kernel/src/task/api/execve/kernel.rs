use crate::{
    prelude::*,
    task::{cpu_usage::Privilege, execve::binfmt::dispatch_execve, futex::exit_robust_list},
};

/// **This function must be run in a process context.**
///
/// `path` is not a global namespace path, but rather a path relative to the
/// current process's filesystem context. **This is important for security and
/// isolation.**
///
/// TODO: 'dethread' current task. if this is not leader thread, we should steal
/// leader's tid.
pub fn kernel_execve(
    path: &impl AsRef<str>,
    argv: &[impl AsRef<str>],
    envp: &[impl AsRef<str>],
) -> Result<(), SysError> {
    let path = path.as_ref();
    let resolved = get_current_task()
        .lookup_path(Path::new(path), ResolveFlags::empty())
        .map_err(|e| {
            knoticeln!("execve: failed to resolve path '{}': {:?}", path, e);
            e
        })?;
    kernel_execve_from_pathref(path, resolved, argv, envp)
}

/// **This function must be run in a process context.**
///
/// `exec_fn` is the filename exposed to the new image through argv/auxv, while
/// `path` is the already resolved executable object.
pub fn kernel_execve_from_pathref(
    exec_fn: &str,
    path: PathRef,
    argv: &[impl AsRef<str>],
    envp: &[impl AsRef<str>],
) -> Result<(), SysError> {
    let task = get_current_task();
    let mut old_uspace = task.try_clone_uspace_handle();
    let tgid = task.tgid();

    let mut usp = UserSpace::new()?;
    match dispatch_execve(
        &mut usp,
        exec_fn,
        path,
        argv,
        envp,
        &task.cred(),
        task.no_new_privs(),
    ) {
        Ok(meta) => {
            let new_cred = meta.cred;
            let usp = Arc::new(UserSpaceHandle::new(usp, meta.exe));
            unsafe {
                if !task.flags().is_kernel() {
                    if let Err(e) = exit_robust_list() {
                        knoticeln!(
                            "failed to exit robust list for task {}: {:?}",
                            task.tid(),
                            e,
                        );
                    }
                }

                task.set_clear_child_tid(None);
                task.set_robust_list(None);

                task.dethread();

                // these resoureces must be cleaned after dethreading.
                task.close_cloexec_fds();

                task.sig_disposition.write().clear_custom_actions();
                task.sig_altstack.lock().take();
                // mask, pending stay unchanged.

                if let Some(old_uspace) = old_uspace.take() {
                    if task.is_last_user_of_uspace(&old_uspace) {
                        old_uspace.detach_all_sysv_shm_for(tgid);
                    }
                }

                task.get_thread_group().mark_executed();
                task.replace_cred(new_cred);

                // this must be a user task.
                let name_part = exec_fn.split('/').last().unwrap_or(exec_fn);
                let name = (String::from("@user/") + name_part).into_boxed_str();
                let flags = TaskFlags::empty();

                IntrArch::local_intr_disable();

                // this operation must be placed after dethreading.
                // dethreading possibly triggers yield, which will change mapping to old
                // uspace!!!
                usp.activate();
                task.switch_exec_ctx(name, usp, flags, false);

                let ksp = task.kstack().stack_top();
                task.on_prv_change(Privilege::User);
                task.vfork_done.publish(1, true);

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
