For simplicity, in vfs subsystem we use a big-lock pattern. for example, a superblock struct may have a huge lock that protects all mutable fields.

we may introduce finer-grained locks in the future, but for now we do so to speed up development and avoid deadlocks.

VFS itself relies heavily on runtime polimorphism, which is tough to fully abstract into a static type system. Therefore, we tent to use various wrappers to communicate the semantic of an entity. for example, we have raw [Inode] struct, and wapper types: [DirView], [RegularView], etc. these wrappers are created at runtime, ensuring the correct type of the underlying inode, and provide a more specific API for the caller. This way we can have a more flexible design while still maintaining some level of type safety.