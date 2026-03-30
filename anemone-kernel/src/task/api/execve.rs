use alloc::sync::Arc;

use crate::{
    prelude::{image::UserTaskImage, *},
    utils::align::PhantomAligned64,
};

pub fn kernel_execve_from_image(
    elf_image: UserTaskImage,
    commandline: impl AsRef<str>,
    argv: impl IntoIterator<Item = impl AsRef<str>>,
    envp: impl IntoIterator<Item = impl AsRef<str>>,
) -> Result<(), SysError> {
    let memsp = Arc::new(elf_image.memsp);
    let insert_args = || -> Result<VirtAddr, MmError> {
        let mut len: u64 = 0;
        for arg in argv.into_iter() {
            let _ = unsafe {
                memsp.push_to_init_stack::<u8>(arg.as_ref().as_bytes())?;
            };
            len += 1;
        }
        unsafe {
            // 64-bytes aligned length
            return Ok(memsp.push_to_init_stack::<PhantomAligned64>(&u64::to_le_bytes(len))?);
        }
    };
    let sp = match insert_args() {
        Ok(arg) => arg,
        Err(e) => {
            unsafe {
                memsp.clear_stack();
            }
            return Err(e.into());
        },
    };
    let mut ksp = VirtAddr::new(0);
    with_current_task(|task| {
        let info = TaskInfo {
            cmdline: commandline.as_ref().into(),
            flags: TaskFlags::NONE,
            uspace: Some(memsp.clone()),
        };
        unsafe {
            task.set_info(info);
        }
        ksp = task.kstack().stack_top();
    });
    unsafe {
        IntrArch::local_intr_disable();
        memsp.activate();

        load_context(TaskContext::from_user_fn(
            VirtAddr::new(elf_image.entry as u64),
            sp,
            ksp,
        ));
    }
    unreachable!("should never return to a wasted context");
}
