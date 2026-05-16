#![no_std]
#![no_main]
#![feature(f128)]
#![feature(f16)]

use anemone_rs::prelude::*;
use core::hint::black_box;

macro_rules! test_for_type {
    ($type: tt, $a: expr, $b: expr) => {
        println!("----- testing type: {} -----", stringify!($type));
        let a: $type = $a;
        let b: $type = $b;
        println!("a = {:?}, b = {:?}", a, b);
        println!("{:?} + {:?} = {:?}", a, b, black_box(a + b));
        println!("{:?} - {:?} = {:?}", a, b, black_box(a - b));
        println!("{:?} * {:?} = {:?}", a, b, black_box(a * b));
        println!("{:?} / {:?} = {:?}", a, b, black_box(a / b));
    };
}

macro_rules! test_all {
    ($name: expr,$a: expr, $b: expr) => {
        println!("===== testing {} =====", $name);
        test_for_type!(f32, $a, $b);
        test_for_type!(f64, $a, $b);
        test_for_type!(f16, $a, $b);
        test_for_type!(f128, $a, $b);
    };
}

#[anemone_rs::main]
pub fn main() -> Result<(), Errno> {
    println!("===== float test version 4 =====");
    test_for_type!(f32, 1.234567, 7.890123);
    test_for_type!(f64, 1.23456789012345, 7.89012345678901);
    test_for_type!(f16, 3.33, 4.444);
    test_for_type!(
        f128,
        1.23456789012345678901234567890123,
        7.89012345678901234567890123456789
    );

    test_all!("invalid operation", 0.0, 0.0);

    test_all!("divide by zero", 1.0, 0.0);

    test_all!("not exact", 1.0, 3.0);

    println!("===== testing overflow =====");
    println!(
        "f16: MAX({}) + 200 = {:?}",
        f16::MAX,
        black_box(f16::MAX * 1.1)
    );
    println!(
        "f32: MAX({}) + 200 = {:?}",
        f32::MAX,
        black_box(f32::MAX * 1.1)
    );
    println!(
        "f64: MAX({}) + 200 = {:?}",
        f64::MAX,
        black_box(f64::MAX * 1.1)
    );
    println!(
        "f128: MAX({:?}) + 200 = {:?}",
        f128::MAX,
        black_box(f128::MAX * 1.1)
    );

    println!("===== testing underflow =====");
    println!(
        "f16: MIN_POSITIVE({}) / 2 = {:?}",
        black_box(f16::MIN_POSITIVE),
        black_box(f16::MIN_POSITIVE / 2.0)
    );
    println!(
        "f32: MIN_POSITIVE({}) / 2 = {:?}",
        black_box(f32::MIN_POSITIVE),
        black_box(f32::MIN_POSITIVE / 2.0)
    );
    println!(
        "f64: MIN_POSITIVE({}) / 2 = {:?}",
        black_box(f64::MIN_POSITIVE),
        black_box(f64::MIN_POSITIVE / 2.0)
    );
    println!(
        "f128: MIN_POSITIVE({:?}) / 2 = {:?}",
        black_box(f128::MIN_POSITIVE),
        black_box(f128::MIN_POSITIVE / 2.0)
    );

    println!("===== float test passed =====");
    Ok(())
}
