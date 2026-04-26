mod audio;
mod cli;
mod server;
mod session;
mod signal;
mod webrtc_engine;

use std::net::TcpListener;

use anyhow::Context;
use clap::Parser;
use cpal::traits::DeviceTrait;
use serde::Serialize;
use tracing::{error, info};
use tracing_subscriber::EnvFilter;

use crate::{
    audio::{choose_stream_config, list_input_devices, resolve_input_device},
    cli::{Cli, Command},
    server::{run_host, HostConfig},
};

#[tokio::main]
async fn main() {
    if let Err(err) = run().await {
        error!("{err:#}");
        std::process::exit(1);
    }
}

async fn run() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("sillymic_host=info,webrtc=warn")),
        )
        .with_target(false)
        .compact()
        .init();

    let cli = Cli::parse();

    match cli.command {
        Command::Host { port, pin, input } => {
            run_host(HostConfig {
                port,
                pin,
                input_selector: input,
            })
            .await?;
        }
        Command::Devices => {
            let devices = list_input_devices()?;
            if devices.is_empty() {
                println!("No input devices found.");
            } else {
                println!("Available input devices:");
                for d in devices {
                    let marker = if d.default { " (default)" } else { "" };
                    println!("  [{}] {}{}", d.index, d.name, marker);
                }
            }
        }
        Command::Doctor { port, input } => run_doctor(port, input.as_deref())?,
    }

    Ok(())
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
enum DoctorStatus {
    Pass,
    Warn,
    Fail,
}

#[derive(Debug, Clone, Serialize)]
struct DoctorCheck {
    code: &'static str,
    status: DoctorStatus,
    message: String,
}

#[derive(Debug, Clone, Serialize)]
struct DoctorReport {
    checks: Vec<DoctorCheck>,
}

fn run_doctor(port: u16, input_selector: Option<&str>) -> anyhow::Result<()> {
    let mut checks = Vec::new();

    match local_ip_address::local_ip() {
        Ok(ip) => checks.push(DoctorCheck {
            code: "DR001_LAN_IP",
            status: DoctorStatus::Pass,
            message: format!("Detected local IP address: {ip}"),
        }),
        Err(err) => checks.push(DoctorCheck {
            code: "DR001_LAN_IP",
            status: DoctorStatus::Warn,
            message: format!("Could not determine LAN IP: {err}"),
        }),
    }

    match resolve_input_device(input_selector) {
        Ok(device) => {
            let device_name = device.name().unwrap_or_else(|_| "<unknown>".to_string());
            match choose_stream_config(&device) {
                Ok(cfg) => checks.push(DoctorCheck {
                    code: "DR002_INPUT_DEVICE",
                    status: DoctorStatus::Pass,
                    message: format!(
                        "Input OK on '{device_name}' ({:?}, {}Hz, {}ch)",
                        cfg.sample_format(),
                        cfg.sample_rate().0,
                        cfg.channels()
                    ),
                }),
                Err(err) => checks.push(DoctorCheck {
                    code: "DR002_INPUT_DEVICE",
                    status: DoctorStatus::Fail,
                    message: format!("Input device exists but has no valid stream config: {err}"),
                }),
            }
        }
        Err(err) => checks.push(DoctorCheck {
            code: "DR002_INPUT_DEVICE",
            status: DoctorStatus::Fail,
            message: format!("Input device unavailable: {err}"),
        }),
    }

    match TcpListener::bind(("0.0.0.0", port)) {
        Ok(listener) => {
            drop(listener);
            checks.push(DoctorCheck {
                code: "DR003_PORT_BIND",
                status: DoctorStatus::Pass,
                message: format!("Port {port} is available"),
            });
        }
        Err(err) => checks.push(DoctorCheck {
            code: "DR003_PORT_BIND",
            status: DoctorStatus::Fail,
            message: format!("Port {port} unavailable: {err}"),
        }),
    }

    let report = DoctorReport { checks };
    let pretty = serde_json::to_string_pretty(&report).context("Could not render doctor report")?;
    println!("{pretty}");
    info!("Doctor completed");
    Ok(())
}
