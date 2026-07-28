if(NOT DEFINED ENV{ARCH})
    set(ARCH "x86_64")
else()
    set(ARCH $ENV{ARCH})
endif()

if(DEFINED ENV{LWEXT4_SYSROOT})
    set(LWEXT4_SYSROOT $ENV{LWEXT4_SYSROOT})
else()
    set(LWEXT4_SYSROOT "")
endif()

# Name of the target
set(CMAKE_SYSTEM_NAME "Linux")
set(CMAKE_SYSTEM_PROCESSOR ${ARCH})
set(CMAKE_TRY_COMPILE_TARGET_TYPE STATIC_LIBRARY)

# Toolchain settings
set(TOOLCHAIN_PREFIX ${ARCH}-linux-musl)

if(DEFINED ENV{CC})
    set(CMAKE_C_COMPILER $ENV{CC})
else()
    set(CMAKE_C_COMPILER ${TOOLCHAIN_PREFIX}-cc)
endif()

if(DEFINED ENV{CXX})
    set(CMAKE_CXX_COMPILER $ENV{CXX})
else()
    set(CMAKE_CXX_COMPILER ${TOOLCHAIN_PREFIX}-c++)
endif()

set(AS ${TOOLCHAIN_PREFIX}-as)

if(DEFINED ENV{AR})
    set(AR $ENV{AR})
else()
    set(AR ${TOOLCHAIN_PREFIX}-ar)
endif()

if(DEFINED ENV{OBJCOPY})
    set(OBJCOPY $ENV{OBJCOPY})
else()
    set(OBJCOPY ${TOOLCHAIN_PREFIX}-objcopy)
endif()

if(DEFINED ENV{OBJDUMP})
    set(OBJDUMP $ENV{OBJDUMP})
else()
    set(OBJDUMP ${TOOLCHAIN_PREFIX}-objdump)
endif()

if(DEFINED ENV{SIZE})
    set(SIZE $ENV{SIZE})
else()
    set(SIZE ${TOOLCHAIN_PREFIX}-size)
endif()

if(NOT LWEXT4_SYSROOT STREQUAL "")
    set(CMAKE_SYSROOT ${LWEXT4_SYSROOT})
endif()

set(LD_FLAGS "-nolibc -nostdlib -static --gc-sections -nostartfiles")

set(CMAKE_C_FLAGS "-std=gnu99 -fdata-sections -ffunction-sections" CACHE INTERNAL "c compiler flags")
set(CMAKE_CXX_FLAGS "-fdata-sections -ffunction-sections" CACHE INTERNAL "cxx compiler flags")
set(CMAKE_ASM_FLAGS "" CACHE INTERNAL "asm compiler flags")

if(ARCH STREQUAL "x86_64")
    set(ARCH_C_FLAGS "-mno-sse")
elseif(ARCH STREQUAL "aarch64")
    set(ARCH_C_FLAGS "-mgeneral-regs-only")
elseif(ARCH STREQUAL "riscv64")
    set(ARCH_C_FLAGS "-march=rv64gc -mabi=lp64d -mcmodel=medany")
elseif(ARCH STREQUAL "loongarch64")
    # Static kernel symbols cannot be interposed, so direct external access emits PC-relative relocations instead of GOT loads.
    # Match the kernel target's -ual contract: LS2K traps on widened loads or stores that are not naturally aligned.
    set(ARCH_C_FLAGS "-mabi=lp64d -mdirect-extern-access -mstrict-align")
else()
    set(ARCH_C_FLAGS "")
endif()

set(CMAKE_C_FLAGS "-fno-PIC -fno-builtin -ffreestanding -fno-omit-frame-pointer ${CMAKE_C_FLAGS} ${ARCH_C_FLAGS}")
set(CMAKE_CXX_FLAGS "-fno-PIC -nostdinc -fno-builtin -ffreestanding -fno-omit-frame-pointer ${CMAKE_CXX_FLAGS} ${ARCH_C_FLAGS}")

if(APPLE)
    set(CMAKE_EXE_LINKER_FLAGS "-dead_strip" CACHE INTERNAL "exe link flags")
else(APPLE)
    set(CMAKE_EXE_LINKER_FLAGS "-Wl,--gc-sections" CACHE INTERNAL "exe link flags")
endif(APPLE)

SET(CMAKE_C_FLAGS_DEBUG "-O0 -g -ggdb3" CACHE INTERNAL "c debug compiler flags")
SET(CMAKE_CXX_FLAGS_DEBUG "-O0 -g -ggdb3" CACHE INTERNAL "cxx debug compiler flags")
SET(CMAKE_ASM_FLAGS_DEBUG "-g -ggdb3" CACHE INTERNAL "asm debug compiler flags")

SET(CMAKE_C_FLAGS_RELEASE "-O2 -g -ggdb3" CACHE INTERNAL "c release compiler flags")
SET(CMAKE_CXX_FLAGS_RELEASE "-O2 -g -ggdb3" CACHE INTERNAL "cxx release compiler flags")
SET(CMAKE_ASM_FLAGS_RELEASE "" CACHE INTERNAL "asm release compiler flags")
