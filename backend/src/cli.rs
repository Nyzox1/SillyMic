use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(
    name = "sillymic",
    version,
    about = "LAN microphone bridge from PC to iPhone"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Start the host server and stream microphone to one mobile client.
    Host {
        #[arg(long, default_value_t = 41777)]
        port: u16,
        #[arg(long, value_parser = validate_pin)]
        pin: String,
        #[arg(long)]
        input: Option<String>,
    },
    /// List available input devices.
    Devices,
    /// Run environment diagnostics.
    Doctor {
        #[arg(long, default_value_t = 41777)]
        port: u16,
        #[arg(long)]
        input: Option<String>,
    },
}

fn validate_pin(v: &str) -> Result<String, String> {
    if v.len() != 6 || !v.chars().all(|c| c.is_ascii_digit()) {
        return Err("PIN must contain exactly 6 digits".to_string());
    }
    Ok(v.to_string())
}
