use clap::Args;
use std::fs;
use tracing::{info, warn};

#[derive(Args)]
pub struct DownloadArgs {
    /// model path to check.
    #[clap(long)]
    model: String,
}

pub async fn main(args: &DownloadArgs) {
    if fs::metadata(&args.model).is_ok() {
        info!("Model exists at local path: {}", args.model);
    } else {
        warn!("Model not found at local path: {}", args.model);
    }
}
