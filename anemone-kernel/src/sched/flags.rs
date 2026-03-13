//类似linux的设计，根据传入的信号确定是创建进程（fork）还是线程（clone）
bitflags::bitflags! {
    pub struct TaskCloneFlags: u32 {
        const CLONE_VM = 0x00000100;      // 共享地址空间，
        const CLONE_FILES = 0x00000400;   // 共享文件描述符表， 作为线程创建的时候应该包括这两个信号
    //目前这两个信号应该够用
    }
}