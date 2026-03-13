This software is written according to the Device Tree Specification, version 0.4, primarily targeting FDT format version 17, with backwards compatibility for version 16. For more information on the specification, refer to the official documentation at https://github.com/devicetree-org/devicetree-specification/releases/tag/v0.4.

This library is intended to work with operating system kernels, and thus panic immediately on any errors, since there is no way for kernel code to recover from them during early boot. Also, we assumes that once the device tree is parsed and unflattened, it will last for the entire lifetime of the kernel.

All kernel needs to use this library is to provide an arena area large enough to hold the unflattened device tree in memory.

Some of the APIs depend on `alloc` features, which will be explicitly marked in documentation. However, the core parsing logic and unflattening logic do not require `alloc`, and can be used in `no_std` environments without an allocator.

TODO:
- Add documentation here and comment the code in the crate. For now, see the source code for details.
- Runtime modifiability for hotplugging and other dynamic device management scenarios. 