use std::path::PathBuf;
use clap::Parser;
#[derive(Parser, Clone)]
pub struct Config {
    #[clap(default_value = "")]
    pub path: PathBuf,

    #[clap(long, default_value_t = 4)]
    pub preload: usize,
}
