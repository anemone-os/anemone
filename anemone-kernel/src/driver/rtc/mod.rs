//! Real time clock (RTC) driver.
//!
//! RTCs provide a persistent time source that can be used to maintain the
//! system time across reboots. They are typically battery-backed, allowing them
//! to keep time even when the system is powered off. RTCs can also provide
//! alarm functionality, allowing the system to wake up at a specified time.

pub mod goldfish;
