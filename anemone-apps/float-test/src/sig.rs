use core::{
    arch::asm,
    hint::{black_box, spin_loop},
    sync::atomic::AtomicBool,
};

use anemone_rs::{
    abi::process::linux::{
        signal::{SA_SIGINFO, SigAction, SigInfo, SigSet},
        ucontext::UContext,
    },
    os::linux::process::{
        getpid, gettid,
        signal::{SigNo, sigaction, tgkill},
    },
    prelude::*,
};

static HANDLED: AtomicBool = AtomicBool::new(false);

macro_rules! float_value {
    ($val:expr) => {
        const { $val.to_bits() }
    };
}

pub fn sig_usr1_handler(signo: SigNo, info: *const SigInfo, ucontext: *const UContext) {
    let ra: u64;
    #[cfg(target_arch = "riscv64")]
    unsafe {
        asm!("mv {0}, ra", out(reg) ra);
    }
    #[cfg(target_arch = "loongarch64")]
    unsafe {
        asm!("move {0}, $ra", out(reg) ra);
    }
    println!("[SIG] ra = {:#x}", ra);
    println!(
        "[SIG] in signal handler for signal {}, siginfo at {:p}, ucontext at {:p}",
        signo.as_usize(),
        info,
        ucontext
    );
    HANDLED.store(true, core::sync::atomic::Ordering::SeqCst);
    #[cfg(target_arch = "riscv64")]
    unsafe {
        asm!("fmv.d.x f0, {0}", in(reg) float_value!(666f64));
        asm!("fmv.d.x f1, {0}", in(reg) float_value!(233f64));
    }
    #[cfg(target_arch = "loongarch64")]
    unsafe {
        asm!("movgr2fr.d $fa0, {0}", in(reg) float_value!(666f64));
        asm!("movgr2fr.d $fa1, {0}", in(reg) float_value!(233f64));
    }
    let a: u64;
    let b: u64;
    // modify the floating point registers in the signal handler
    #[cfg(target_arch = "riscv64")]
    unsafe {
        asm!("fmv.x.d {0}, f0", out(reg) a);
        asm!("fmv.x.d {0}, f1", out(reg) b);
    }
    #[cfg(target_arch = "loongarch64")]
    unsafe {
        asm!("movfr2gr.d {0}, $fa0", out(reg) a);
        asm!("movfr2gr.d {0}, $fa1", out(reg) b);
    }
    println!(
        "[SIG] in signal handler, fa0={}, fa1={}",
        f64::from_bits(a),
        f64::from_bits(b)
    );
    println!("[SIG] signal handler done");
}

pub fn run() {
    println!("----- running signal handler test -----");
    // test 1: enable floatpoint on signal handler
    let action = SigAction {
        sighandler: sig_usr1_handler as *const (),
        sa_flags: SA_SIGINFO,
        sa_mask: SigSet { bits: 0 },
    };
    sigaction(SigNo::SIGUSR1, Some(&action), None).expect("fatal: failed to register sigaction");
    tgkill(
        getpid().expect("failed to get pid"),
        gettid().expect("failed to get tid"),
        SigNo::SIGUSR1,
    )
    .expect("failed to raise signal");
    while !HANDLED.load(core::sync::atomic::Ordering::SeqCst) {
        spin_loop();
    }
    // double test 1
    let a = 0.114514;
    let b = 1919.810;
    let res = black_box(a) + black_box(b);
    println!("{}+{}={}", a, b, res);
    assert!(res == const { 0.114514 + 1919.810 });
    run_ucontext_test();
    println!("----- all tests passed -----");
}

pub fn run_ucontext_test() {
    unsafe {
        #[cfg(target_arch = "riscv64")]
        {
            asm!("fmv.d.x f0, {0}", in(reg) float_value!(114514f64));
            asm!("fmv.d.x f1, {0}", in(reg) float_value!(1919810f64));
        }
        #[cfg(target_arch = "loongarch64")]
        {
            asm!("movgr2fr.d $fa0, {0}", in(reg) float_value!(114514f64));
            asm!("movgr2fr.d $fa1, {0}", in(reg) float_value!(1919810f64));
        }
        HANDLED.store(false, core::sync::atomic::Ordering::SeqCst);
        tgkill(
            getpid().expect("failed to get pid"),
            gettid().expect("failed to get tid"),
            SigNo::SIGUSR1,
        )
        .expect("failed to raise signal");
        while !HANDLED.load(core::sync::atomic::Ordering::SeqCst) {
            spin_loop();
        }
        let a: u64;
        let b: u64;
        #[cfg(target_arch = "riscv64")]
        {
            asm!("fmv.x.d {0}, f0", out(reg) a);
            asm!("fmv.x.d {0}, f1", out(reg) b);
        }
        #[cfg(target_arch = "loongarch64")]
        {
            asm!("movfr2gr.d {0}, $fa0", out(reg) a);
            asm!("movfr2gr.d {0}, $fa1", out(reg) b);
        }
        let a = f64::from_bits(a);
        let b = f64::from_bits(b);
        println!("After signal handler, fa0={}, fa1={}", a, b);
        assert!(a == const { 114514f64 });
        assert!(b == const { 1919810f64 });
    }
}
