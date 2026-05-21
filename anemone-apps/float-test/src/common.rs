use anemone_rs::prelude::*;
use core::ops::{Add, Div, Mul, Sub};

// ============================================================
// ANSI colors
// ============================================================

// ============================================================
// Const fn: Fibonacci-like seed generator (per type, no fn ptr)
// ============================================================

const fn fib_f64(a0: f64, a1: f64) -> [f64; 20] {
    let mut arr = [0.0f64; 20];
    arr[0] = a0;
    arr[1] = a1;
    let mut i = 2;
    while i < 20 {
        arr[i] = arr[i - 1] + arr[i - 2];
        i += 1;
    }
    arr
}

const fn fib_f32(a0: f32, a1: f32) -> [f32; 20] {
    let mut arr = [0.0f32; 20];
    arr[0] = a0;
    arr[1] = a1;
    let mut i = 2;
    while i < 20 {
        arr[i] = arr[i - 1] + arr[i - 2];
        i += 1;
    }
    arr
}

// ============================================================
// Seed values
// ============================================================

const A_F64: [f64; 20] = fib_f64(1.2345678901234567, 1.1118282189289192);
const B_F64: [f64; 20] = fib_f64(9.876543210987654, 782.2143287432437);
const A_F32: [f32; 20] = fib_f32(1.2345679, 1.1118282);
const B_F32: [f32; 20] = fib_f32(9.876543, 782.21436);

// ============================================================
// Pre-computed arithmetic test tables
// ============================================================

#[derive(Copy, Clone)]
struct Arith<T> {
    seed: u32,
    a: T,
    b: T,
    add: T,
    sub: T,
    mul: T,
    div: T,
}

const fn build_arith_f64(a_seeds: &[f64; 20], b_seeds: &[f64; 20]) -> [Arith<f64>; 20] {
    let zero = Arith {
        seed: 0,
        a: 0.0,
        b: 0.0,
        add: 0.0,
        sub: 0.0,
        mul: 0.0,
        div: 0.0,
    };
    let mut cases = [zero; 20];
    let mut i = 0;
    while i < 20 {
        let a = a_seeds[i];
        let b = b_seeds[i];
        cases[i] = Arith {
            seed: (i + 1) as u32,
            a,
            b,
            add: a + b,
            sub: a - b,
            mul: a * b,
            div: a / b,
        };
        i += 1;
    }
    cases
}

const fn build_arith_f32(a_seeds: &[f32; 20], b_seeds: &[f32; 20]) -> [Arith<f32>; 20] {
    let zero = Arith {
        seed: 0,
        a: 0.0,
        b: 0.0,
        add: 0.0,
        sub: 0.0,
        mul: 0.0,
        div: 0.0,
    };
    let mut cases = [zero; 20];
    let mut i = 0;
    while i < 20 {
        let a = a_seeds[i];
        let b = b_seeds[i];
        cases[i] = Arith {
            seed: (i + 1) as u32,
            a,
            b,
            add: a + b,
            sub: a - b,
            mul: a * b,
            div: a / b,
        };
        i += 1;
    }
    cases
}

static ARITH_F64: [Arith<f64>; 20] = build_arith_f64(&A_F64, &B_F64);
static ARITH_F32: [Arith<f32>; 20] = build_arith_f32(&A_F32, &B_F32);

// ============================================================
// Pre-computed exception test tables (5 IEEE 754 exceptions)
// ============================================================

#[derive(Copy, Clone)]
struct Exn<T> {
    name: &'static str,
    a: T,
    b: T,
    add: T,
    sub: T,
    mul: T,
    div: T,
}

const fn exn_f64() -> [Exn<f64>; 5] {
    let max = f64::MAX;
    let min = f64::MIN_POSITIVE;
    [
        Exn {
            name: "invalid",
            a: 0.0,
            b: 0.0,
            add: 0.0,
            sub: 0.0,
            mul: 0.0,
            div: f64::NAN,
        },
        Exn {
            name: "divide_by_zero",
            a: 1.0,
            b: 0.0,
            add: 1.0,
            sub: 1.0,
            mul: 0.0,
            div: f64::INFINITY,
        },
        Exn {
            name: "inexact",
            a: 1.0,
            b: 3.0,
            add: 4.0,
            sub: -2.0,
            mul: 3.0,
            div: 1.0 / 3.0,
        },
        Exn {
            name: "overflow",
            a: max,
            b: max,
            add: f64::INFINITY,
            sub: max - max,
            mul: f64::INFINITY,
            div: max / max,
        },
        Exn {
            name: "underflow",
            a: min,
            b: 2.0,
            add: min + 2.0,
            sub: min - 2.0,
            mul: min * 2.0,
            div: min / 2.0,
        },
    ]
}

const fn exn_f32() -> [Exn<f32>; 5] {
    let max = f32::MAX;
    let min = f32::MIN_POSITIVE;
    [
        Exn {
            name: "invalid",
            a: 0.0,
            b: 0.0,
            add: 0.0,
            sub: 0.0,
            mul: 0.0,
            div: f32::NAN,
        },
        Exn {
            name: "divide_by_zero",
            a: 1.0,
            b: 0.0,
            add: 1.0,
            sub: 1.0,
            mul: 0.0,
            div: f32::INFINITY,
        },
        Exn {
            name: "inexact",
            a: 1.0,
            b: 3.0,
            add: 4.0,
            sub: -2.0,
            mul: 3.0,
            div: 1.0 / 3.0,
        },
        Exn {
            name: "overflow",
            a: max,
            b: max,
            add: f32::INFINITY,
            sub: max - max,
            mul: f32::INFINITY,
            div: max / max,
        },
        Exn {
            name: "underflow",
            a: min,
            b: 2.0,
            add: min + 2.0,
            sub: min - 2.0,
            mul: min * 2.0,
            div: min / 2.0,
        },
    ]
}

static EXN_F64: [Exn<f64>; 5] = exn_f64();
static EXN_F32: [Exn<f32>; 5] = exn_f32();

// ============================================================
// ULP comparison (±1 ULP for rounding-mode differences)
// ============================================================

fn within_1ulp_f64(expected: f64, actual: f64) -> bool {
    let e = expected.to_bits();
    let a = actual.to_bits();
    if e == a {
        return true;
    }
    if expected.is_nan() {
        return actual.is_nan();
    }
    if expected == 0.0 && actual == 0.0 {
        return true;
    }
    if expected.is_infinite() || actual.is_infinite() {
        return false;
    }
    if (e >> 63) != (a >> 63) {
        return false;
    }
    (if e > a { e - a } else { a - e }) <= 1
}

fn within_1ulp_f32(expected: f32, actual: f32) -> bool {
    let e = expected.to_bits();
    let a = actual.to_bits();
    if e == a {
        return true;
    }
    if expected.is_nan() {
        return actual.is_nan();
    }
    if expected == 0.0 && actual == 0.0 {
        return true;
    }
    if expected.is_infinite() || actual.is_infinite() {
        return false;
    }
    if (e >> 31) != (a >> 31) {
        return false;
    }
    (if e > a { e - a } else { a - e }) <= 1
}

// ============================================================
// Check & panic
// ============================================================

fn check_f64(expected: f64, actual: f64, label: &str, seed: u32, op: &str) {
    if within_1ulp_f64(expected, actual) {
        println!(
            "(expected = {:?}, actual = {:?}) {}",
            expected, actual, "\x1b[32mok\x1b[0m"
        );
    } else {
        println!("\x1b[31merror\x1b[0m");
        panic!(
            "[f64] seed={} {}: {} {} {} => expected {} (0x{:016x}), got {} (0x{:016x})",
            seed,
            label,
            expected,
            op,
            actual,
            expected,
            expected.to_bits(),
            actual,
            actual.to_bits(),
        );
    }
}

fn check_f32(expected: f32, actual: f32, label: &str, seed: u32, op: &str) {
    if within_1ulp_f32(expected, actual) {
        println!(
            "(expected = {:?}, actual = {:?}) {}",
            expected, actual, "\x1b[32mok\x1b[0m"
        );
    } else {
        println!("\x1b[31merror\x1b[0m");
        panic!(
            "[f32] seed={} {}: {} {} {} => expected {} (0x{:08x}), got {} (0x{:08x})",
            seed,
            label,
            expected,
            op,
            actual,
            expected,
            expected.to_bits(),
            actual,
            actual.to_bits(),
        );
    }
}

// ============================================================
// Test runner
// ============================================================

fn run_arith<
    T: Copy + core::fmt::Debug + Add<Output = T> + Sub<Output = T> + Mul<Output = T> + Div<Output = T>,
>(
    cases: &[Arith<T>],
    check: fn(T, T, &str, u32, &str),
    tname: &str,
) {
    for case in cases {
        let a = case.a;
        let b = case.b;
        let bb = core::hint::black_box;

        println!("[{}] seed={} add: {:?} + {:?}", tname, case.seed, a, b);
        check(case.add, bb(a) + bb(b), "add", case.seed, "+");

        println!("[{}] seed={} sub: {:?} - {:?}", tname, case.seed, a, b);
        check(case.sub, bb(a) - bb(b), "sub", case.seed, "-");

        println!("[{}] seed={} mul: {:?} * {:?}", tname, case.seed, a, b);
        check(case.mul, bb(a) * bb(b), "mul", case.seed, "*");

        println!("[{}] seed={} div: {:?} / {:?}", tname, case.seed, a, b);
        check(case.div, bb(a) / bb(b), "div", case.seed, "/");
    }
}

fn run_exn<
    T: Copy + core::fmt::Debug + Add<Output = T> + Sub<Output = T> + Mul<Output = T> + Div<Output = T>,
>(
    cases: &[Exn<T>],
    check: fn(T, T, &str, u32, &str),
    tname: &str,
) {
    for case in cases {
        let a = case.a;
        let b = case.b;
        let bb = core::hint::black_box;

        println!("[{}] {} add: {:?} + {:?}", tname, case.name, a, b);
        check(case.add, bb(a) + bb(b), case.name, 0, "+");

        println!("[{}] {} sub: {:?} - {:?}", tname, case.name, a, b);
        check(case.sub, bb(a) - bb(b), case.name, 0, "-");

        println!("[{}] {} mul: {:?} * {:?}", tname, case.name, a, b);
        check(case.mul, bb(a) * bb(b), case.name, 0, "*");

        println!("[{}] {} div: {:?} / {:?}", tname, case.name, a, b);
        check(case.div, bb(a) / bb(b), case.name, 0, "/");
    }
}

pub fn run_seed_range(start: u32, end: u32) {
    if start > 20 || end > 20 || start > end {
        panic!("seed range {}-{} out of bounds (1-20)", start, end);
    }
    let lo = (start - 1) as usize;
    let hi = end as usize;

    println!("===== f32 arithmetic tests (seeds {}-{}) =====", start, end);
    run_arith(&ARITH_F32[lo..hi], check_f32, "f32");

    println!("===== f64 arithmetic tests (seeds {}-{}) =====", start, end);
    run_arith(&ARITH_F64[lo..hi], check_f64, "f64");

    println!("===== f32 exception tests =====");
    run_exn(&EXN_F32, check_f32, "f32");

    println!("===== f64 exception tests =====");
    run_exn(&EXN_F64, check_f64, "f64");

    println!("===== all float tests passed =====");
}
