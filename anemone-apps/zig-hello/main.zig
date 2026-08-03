const std = @import("std");
const c = @cImport({
    @cInclude("termios.h");
});

const anemone_power_shutdown: c_long = 0x201;
const anemone_shutdown_magic: c_ulong = 0xdeadce11;

extern "c" fn syscall(number: c_long, ...) c_long;
extern "c" fn write(fd: c_int, buffer: [*]const u8, count: usize) isize;

fn writeAll(fd: c_int, text: []const u8) bool {
    var offset: usize = 0;
    while (offset < text.len) {
        const result = write(fd, text.ptr + offset, text.len - offset);
        if (result <= 0) return false;
        offset += @intCast(result);
    }
    return true;
}

fn drainStdout() bool {
    var termios: c.struct_termios = undefined;
    return c.tcgetattr(1, &termios) == 0 and c.tcsetattr(1, c.TCSADRAIN, &termios) == 0;
}

pub fn main() void {
    if (!writeAll(1, "zig-hello: guest execution ok\n") or !drainStdout()) {
        std.process.exit(1);
    }

    // This app is an EmbeddedApp acceptance init, so returning would violate
    // the kernel's init-task lifecycle. These values are the Anemone native
    // power ABI, not Linux ABI; update them with anemone-abi.
    _ = syscall(anemone_power_shutdown, anemone_shutdown_magic);
    _ = writeAll(2, "zig-hello: power shutdown returned unexpectedly\n");
    std.process.exit(1);
}
