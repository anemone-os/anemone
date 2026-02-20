pub fn log_progress(topic: &str, msg: &str) {
    const CYAN_BOLD: &str = "\x1b[1;36m";
    const RESET: &str = "\x1b[0m";
    let width = 12;
    println!(
        "{}{:>width$}{} {}",
        CYAN_BOLD,
        topic,
        RESET,
        msg,
        width = width
    );
}

pub fn warn(topic: &str, msg: &str) {
    const YELLOW_BOLD: &str = "\x1b[1;33m";
    const RESET: &str = "\x1b[0m";
    let width = 12;
    println!(
        "{}{:>width$}{} {}",
        YELLOW_BOLD,
        topic,
        RESET,
        msg,
        width = width
    );
}

#[macro_export]
macro_rules! log_progress {
    ($topic:literal, $msg:expr $(,)?) => {
        $crate::tasks::utils::log_progress($topic, $msg);
    };
}

#[macro_export]
macro_rules! warn {
    ($topic:literal, $msg:expr $(,)?) => {
        $crate::tasks::utils::warn($topic, $msg);
    };
}

// pub fn log_progress(topic: &str, msg: &str) {
//     pretty_log(topic, msg);
// }
