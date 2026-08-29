mod cli;
mod config;
mod protocol;
mod receiver;
mod sender;

use anyhow::Result;
use clap::Parser;
use cli::{Cli, Command};

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Command::Send {
            port,
            bind,
            device,
            verbose,
        } => sender::run(&bind, port, device.as_deref(), verbose).await,
        Command::Recv {
            sender_ip,
            port,
            device,
            verbose,
        } => receiver::run(&sender_ip, port, device.as_deref(), verbose).await,
    }
}
