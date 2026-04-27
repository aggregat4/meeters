use log::{LevelFilter, Metadata, Record};
use std::env;

struct StderrLogger;

static LOGGER: StderrLogger = StderrLogger;

impl log::Log for StderrLogger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        metadata.level() <= log::max_level()
    }

    fn log(&self, record: &Record) {
        if self.enabled(record.metadata()) {
            eprintln!("[{}] {}", record.level(), record.args());
        }
    }

    fn flush(&self) {}
}

pub fn init_from_env() {
    let level = env::var("MEETERS_LOG")
        .ok()
        .as_deref()
        .map(parse_level)
        .unwrap_or(LevelFilter::Warn);

    if log::set_logger(&LOGGER).is_ok() {
        log::set_max_level(level);
    }
}

fn parse_level(value: &str) -> LevelFilter {
    match value.to_ascii_lowercase().as_str() {
        "off" | "0" | "false" => LevelFilter::Off,
        "error" => LevelFilter::Error,
        "warn" | "warning" => LevelFilter::Warn,
        "info" | "1" | "true" => LevelFilter::Info,
        "debug" | "verbose" => LevelFilter::Debug,
        "trace" => LevelFilter::Trace,
        _ => LevelFilter::Warn,
    }
}
