use clap::Parser;
use vrr::viewer::run;
use vrr::config::Config;

fn main() -> anyhow::Result<()> {
    vrr::logger::init();
    let config = Config::parse();
    pollster::block_on(run(config));
    Ok(())
}
