use std::path::PathBuf;
use clap::Parser;
use sniproxy_bin::run;

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Args {
    #[arg(short, long, default_value = "/etc/sniproxy/config.yaml")]
    config: PathBuf,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    run(&args.config).await
}
