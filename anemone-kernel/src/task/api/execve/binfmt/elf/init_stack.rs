//! Construct initial stack.

use anemone_abi::process::linux::aux_vec::AuxvEntry;

use crate::{
    prelude::*,
    task::{
        credentials::CredentialSet,
        execve::binfmt::elf::{
            auxv::{AuxEntry, AuxV},
            parse::ElfMeta,
        },
    },
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum StackBlobKey {
    Arg(usize),
    Env(usize),

    // for auxv. these are difficult to categorize.
    ExecFn,
    Random,
    Platform,
    BasePlatform,
}

pub struct InitStackCtor<'a, T: AsRef<str>, U: AsRef<str>> {
    usp: &'a mut UserSpace,
    meta: &'a ElfMeta,
    exec_fn: &'a str,
    argv: &'a [T],
    env: &'a [U],
    auxv: AuxV,
    /// [StackBlobKey] -> string start virtual address on the initial user
    /// stack.
    record: HashMap<StackBlobKey, VirtAddr>,
}

impl<'a, T: AsRef<str>, U: AsRef<str>> InitStackCtor<'a, T, U> {
    pub fn new(
        usp: &'a mut UserSpace,
        meta: &'a ElfMeta,
        exec_fn: &'a str,
        argv: &'a [T],
        env: &'a [U],
        cred: &CredentialSet,
        secure_exec: bool,
    ) -> Self {
        Self {
            usp,
            meta,
            exec_fn,
            argv,
            env,
            auxv: AuxV::new_partial(cred, secure_exec),
            record: HashMap::new(),
        }
    }

    /// Push all data onto stack, returning the final stack pointer.
    ///
    /// 'all data' includes (from top to bottom):
    /// - argument and environment variable strings
    /// - null-terminating environment variable pointer array
    /// - null-terminating argument pointer array
    /// - 16-bytes aligned argument count (as u64)
    ///
    /// auxv is not implemented yet.
    ///
    /// Internally, this function adopts a multi-pass approach to pushing data:
    /// 1. In the first pass, it pushes all strings and bookkeeps their offsets
    ///    on the initial user stack.
    /// 2. In the second pass, it pushes envp and argv pointer arrays,
    /// 3. Finally, it pushes argc.
    pub fn push(mut self) -> Result<VirtAddr, SysError> {
        self.push_aux_blob()?;

        let pre_env = unsafe { self.usp.current_init_sp() };
        self.push_env_strings()?;
        let after_env = unsafe { self.usp.current_init_sp() };
        unsafe {
            self.usp
                .mark_env_range(after_env, (pre_env - after_env) as usize);
        }

        let pre_cmdline = unsafe { self.usp.current_init_sp() };
        self.push_arg_strings()?;
        let after_cmdline = unsafe { self.usp.current_init_sp() };
        unsafe {
            self.usp
                .mark_cmdline_range(after_cmdline, (pre_cmdline - after_cmdline) as usize);
        }

        // padding down.
        self.prepare_auxv();
        let rest_sz = self.precalc_size();
        let cur_sp = unsafe { self.usp.current_init_sp() };
        let expected_sp = align_down_power_of_2!(cur_sp.get() as usize - rest_sz, 16);
        let padding_sz = (cur_sp.get() as usize - rest_sz)
            .checked_sub(expected_sp)
            .expect("calculation should be correct");
        unsafe {
            static PADDING_BYTES: [u8; 16] = [0; 16];
            self.usp
                .push_to_init_stack::<u8>(&PADDING_BYTES[..padding_sz])?;
        }

        self.push_auxv()?;
        self.push_envp()?;
        self.push_argv()?;

        let final_sp = self.push_argc()?;
        debug_assert!(final_sp.get() % 16 == 0);

        Ok(final_sp)
    }

    /// This only calculates total size of those vectors and argc, excluding
    /// blobs or strings.
    ///
    /// Must be called after [self::prepare_auxv].
    fn precalc_size(&self) -> usize {
        let auxv_sz = self.auxv.len() * size_of::<AuxvEntry>();

        let envp_sz = (self.env.len() + 1) * size_of::<*const u8>();
        let argv_sz = (self.argv.len() + 1) * size_of::<*const u8>();
        let argc_sz = size_of::<u64>();

        auxv_sz + envp_sz + argv_sz + argc_sz
    }

    /// just hard code here. for auxv this is reasonable.
    ///
    /// see [AuxV::new_partial] for details on extra entries we should push
    /// here.
    fn push_aux_blob(&mut self) -> Result<(), SysError> {
        // 1. AT_EXECFN
        let execfn = self.push_c_string_bytes(self.exec_fn.as_bytes())?;
        self.record.insert(StackBlobKey::ExecFn, execfn);

        // 2. AT_RANDOM
        let mut random = [0u8; 16];
        super::auxv::auxv_fill_random_bytes(&mut random);
        let random = unsafe { self.usp.push_to_init_stack::<u8>(&random)? };
        self.record.insert(StackBlobKey::Random, random);

        // 3. AT_PLATFORM & AT_BASE_PLATFORM
        let platform_str = match uts::MACHINE.split_last() {
            Some((&0, bytes)) => bytes,
            _ => uts::MACHINE,
        };
        let platform = self.push_c_string_bytes(platform_str)?;
        self.record.insert(StackBlobKey::Platform, platform);
        self.record.insert(StackBlobKey::BasePlatform, platform);

        Ok(())
    }

    fn push_c_string_bytes(&mut self, bytes: &[u8]) -> Result<VirtAddr, SysError> {
        unsafe {
            self.usp.push_to_init_stack::<u8>(&[0])?;
            self.usp.push_to_init_stack::<u8>(bytes)
        }
    }

    /// Push the rest of auxv entries.
    fn prepare_auxv(&mut self) {
        self.auxv
            .push(AuxEntry::ExecFn(self.record[&StackBlobKey::ExecFn]));
        self.auxv
            .push(AuxEntry::Random(self.record[&StackBlobKey::Random]));
        self.auxv
            .push(AuxEntry::Platform(self.record[&StackBlobKey::Platform]));
        self.auxv.push(AuxEntry::BasePlatform(
            self.record[&StackBlobKey::BasePlatform],
        ));
        self.auxv.push(AuxEntry::Phdr(self.meta.phdrs_addr));
        self.auxv.push(AuxEntry::PhEnt(self.meta.phdr_entry_sz));
        self.auxv.push(AuxEntry::PhNum(self.meta.phdr_entry_num));
        self.auxv.push(AuxEntry::Entry(self.meta.entry));

        if let Some(interp) = &self.meta.interp {
            self.auxv.push(AuxEntry::Base(interp.base));
        }
    }

    // this pushes those key-value pairs.
    fn push_auxv(&mut self) -> Result<(), SysError> {
        // push other auxv entries.
        for entry in self.auxv.iter() {
            let serialized = entry.serialize();
            let raw_bytes = unsafe {
                core::slice::from_raw_parts(
                    &serialized as *const AuxvEntry as *const u8,
                    size_of::<AuxvEntry>(),
                )
            };

            unsafe {
                self.usp.push_to_init_stack::<AuxvEntry>(raw_bytes)?;
            }
        }

        Ok(())
    }

    fn push_env_strings(&mut self) -> Result<(), SysError> {
        for (idx, env) in self.env.iter().enumerate().rev() {
            let env = env.as_ref();
            let bytes = env.as_bytes();
            let offset = unsafe {
                self.usp.push_to_init_stack::<u8>(&[0])?;
                self.usp.push_to_init_stack::<u8>(bytes)?
            };
            self.record.insert(StackBlobKey::Env(idx), offset);
        }

        Ok(())
    }

    fn push_envp(&mut self) -> Result<(), SysError> {
        unsafe {
            self.usp
                .push_to_init_stack::<*const u8>(&0u64.to_ne_bytes())?; // null terminator
        }

        for (idx, _) in self.env.iter().enumerate().rev() {
            // this must succeed since we've already recorded the offset of each env string
            // in the first pass.
            let offset = self.record[&StackBlobKey::Env(idx)];
            unsafe {
                self.usp
                    .push_to_init_stack::<*const u8>(&offset.get().to_ne_bytes())?;
            }
        }
        Ok(())
    }

    fn push_arg_strings(&mut self) -> Result<(), SysError> {
        for (idx, arg) in self.argv.iter().enumerate().rev() {
            let arg = arg.as_ref();
            let bytes = arg.as_bytes();
            let offset = unsafe {
                self.usp.push_to_init_stack::<u8>(&[0])?;
                self.usp.push_to_init_stack::<u8>(bytes)?
            };
            self.record.insert(StackBlobKey::Arg(idx), offset);
        }

        Ok(())
    }

    fn push_argv(&mut self) -> Result<(), SysError> {
        unsafe {
            self.usp
                .push_to_init_stack::<*const u8>(&0u64.to_ne_bytes())?; // null terminator
        }

        for (idx, _) in self.argv.iter().enumerate().rev() {
            // this must succeed since we've already recorded the offset of each arg string
            // in the first pass.
            let offset = self.record[&StackBlobKey::Arg(idx)];
            unsafe {
                self.usp
                    .push_to_init_stack::<*const u8>(&offset.get().to_ne_bytes())?;
            }
        }
        Ok(())
    }

    /// Finally push argc, which should be 16-bytes aligned, and return stack
    /// pointer.
    fn push_argc(&mut self) -> Result<VirtAddr, SysError> {
        let argc = self.argv.len() as u64;
        let offset = unsafe { self.usp.push_to_init_stack::<u64>(&argc.to_ne_bytes())? };
        Ok(offset)
    }
}
