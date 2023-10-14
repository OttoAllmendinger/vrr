use env_logger::Builder;
use log::LevelFilter;
use std::io::Write;
use std::sync::Mutex;
use std::time::Instant;

static START_TIME: Mutex<Option<Instant>> = Mutex::new(None);

pub fn reset_start_time() {
    let mut start_time = START_TIME.lock().unwrap();
    *start_time = Some(Instant::now());
}

pub fn init() {
    reset_start_time();
    Builder::from_default_env()
        .format(move |buf, record| {
            let style = buf.default_level_style(record.level());
            let elapsed_millis = START_TIME
                .lock()
                .unwrap()
                .map(|start_time| start_time.elapsed().as_millis())
                .map_or(String::from(""), |ms| format!("{:6}ms", ms));
            writeln!(
                buf,
                "{} [{} {}] {}",
                elapsed_millis,
                style.value(record.level()),
                record.target(),
                record.args()
            )
        })
        .filter(Some("wgpu_core::device"), LevelFilter::Warn)
        .filter(Some("wgpu_core::present"), LevelFilter::Info)
        .init();
}
