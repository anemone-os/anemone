Buddy system allocator implementation for Anemone kernel, internally storing metadata intrusively within the managed memory region, thus allowing it to be used in a no alloc environment.

**VERIFIED WITH MIRI**: All tests have been verified with Miri to ensure memory safety.

This crate is intentionally decoupled from the rest of the kernel to allow it to be used in a no alloc environment, and to be easily tested and debugged in isolation.

TODO:
- Document and comment the code.
- Add more tests.
- Publish the crate on crates.io.
