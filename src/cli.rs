use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "spozalon")]
#[command(about = "Stream Linux audio to MacBook speakers over Thunderbolt")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
pub enum Command {
    /// Capture system audio and stream it (run on Linux)
    Send {
        #[arg(long, default_value_t = 44100)]
        port: u16,
        #[arg(long, default_value = "0.0.0.0")]
        bind: String,
        #[arg(long)]
        device: Option<String>,
        #[arg(long)]
        verbose: bool,
    },

    /// Receive audio stream and play through speakers (run on macOS)
    Recv {
        /// IP address of the sender
        sender_ip: String,

        /// UDP port to connect to
        #[arg(long, default_value_t = 44100)]
        port: u16,

        /// CoreAudio device to play to (default: system default)
        #[arg(long)]
        device: Option<String>,

        /// Print connection status to stderr
        #[arg(long)]
        verbose: bool,
    },
}
