use alloc::sync::Arc;
use anemone_abi::syscall::SYS_EXECVE;

use crate::prelude::{
    dt::{c_readonly_string, c_readonly_string_array},
    image::{UserTaskImage, load_image_from_file},
    *,
};

#[syscall(SYS_EXECVE)]
pub fn execve(
    #[validate_with(c_readonly_string)] path: Box<str>,
    #[validate_with(c_readonly_string_array)] argv: Vec<Box<str>>,
) -> Result<u64, SysError> {
    let path = Path::new(path.as_ref());
    let path = with_current_task(|task| task.make_global_path(&path));

    kernel_execve(
        &path.to_str().expect("we've already validated path to be a valid C string, whose encoding is a subset of UTF-8"), 
        argv.as_slice()
    )?;
    unreachable!();
}

/// `path` should be an absolute path to an executable file.
pub fn kernel_execve(path: &impl AsRef<str>, argv: &[impl AsRef<str>]) -> Result<(), SysError> {
    let uimage = load_image_from_file(&path)?;
    let mut commandline = argv
        .iter()
        .map(|arg| arg.as_ref())
        .collect::<Vec<_>>()
        .join(" ");
    kernel_execve_from_image(uimage, commandline, argv)?;
    unreachable!();
}

pub fn kernel_execve_from_image(
    elf_image: UserTaskImage,
    commandline: impl AsRef<str>,
    argv: impl IntoIterator<Item = impl AsRef<str>>,
) -> Result<(), SysError> {
    let memsp = Arc::new(elf_image.memsp);
    let mut data = memsp.write();
    let insert_args_fn = || -> Result<VirtAddr, MmError> {
        let mut total_len: u64 = 0;
        // insert strings
        let mut pointers = vec![];
        for arg in argv.into_iter() {
            unsafe { data.push_to_init_stack::<u8>(&0u64.to_ne_bytes())? };
            let pointer = unsafe { data.push_to_init_stack::<u8>(arg.as_ref().as_bytes())? };
            pointers.push(pointer);
            total_len += 1;
        }
        // insert pointers
        unsafe { data.push_to_init_stack::<u64>(&0u64.to_ne_bytes())? };
        for pointer in pointers.iter().rev() {
            unsafe { data.push_to_init_stack::<u64>(&pointer.get().to_ne_bytes())? };
        }
        // insert count
        unsafe {
            // 64-bytes aligned length
            return Ok(data.push_to_init_stack::<u64>(&u64::to_ne_bytes(total_len))?);
        }
    };
    let sp = match insert_args_fn() {
        Ok(arg) => arg,
        Err(e) => {
            unsafe {
                data.clear_stack();
            }
            return Err(e.into());
        },
    };
    drop(data);
    unsafe {
        IntrArch::local_intr_disable();
        memsp.activate();
        let mut ksp = VirtAddr::new(0);
        with_current_task(|task| {
            let info = TaskExecInfo {
                cmdline: commandline.as_ref().into(),
                flags: TaskFlags::NONE,
                uspace: Some(memsp),
            };
            unsafe {
                task.set_exec_info(info);
            }
            ksp = task.kstack().stack_top();
        });
        load_context(TaskContext::from_user_fn(
            VirtAddr::new(elf_image.entry as u64),
            sp,
            ksp,
        ));
    }
    unreachable!("should never return to a wasted context");
}
