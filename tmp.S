.section .text
_start:
    # load 0x8239,5678,8534,1C12 into x10 without using pseudo instructions
    lui x10, 0x82395
    addi x10, x10, -1006 # -1006 is 0xc12 under 2's complement.
    slli x10, x10, 32

    lui x11, 0x85341 # note that 0x85341 is sign-extended.
    addi x12, zero, 0xC12 # so does 0xC12.
    # now erase those bits.
    slli x11, x11, 32
    srli x11, x11, 32
    slli x12, x12, 32
    srli x12, x12, 32

    # ok, we can merge them now.
    or x10, x10, x11
    or x10, x10, x12

    # done.