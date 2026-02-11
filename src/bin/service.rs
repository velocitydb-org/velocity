use clap::{Parser, Subcommand};
use colored::*;
use std::fs;
use std::path::{PathBuf};

#[path = "../config.rs"]
mod config;
#[path = "../service_runner.rs"]
mod service_runner;

use crate::service_runner::{run_velocity_service, ServiceSpec};

#[derive(Parser)]
#[command(name = "velocity-service")]
#[command(about = "Helper for installing/running VelocityDB as a background service.")]
#[command(version)]
struct ServiceCli {
    #[arg(long, default_value = "velocity.toml")]
    config: PathBuf,

    #[arg(long, default_value = "./velocitydb")]
    data_dir: PathBuf,

    #[arg(short, long)]
    bind: Option<String>,

    #[arg(short, long)]
    verbose: bool,

    #[arg(long, default_value = "./service_templates")]
    template_dir: PathBuf,

    #[command(subcommand)]
    command: ServiceCommand,
}

#[derive(Subcommand)]
enum ServiceCommand {
    Run {
        #[arg(long)]
        pid_file: Option<PathBuf>,
    },
    Install {},
    Uninstall {},
}

fn generate_systemd_unit(
    exe_path: &str,
    config: &PathBuf,
    data_dir: &PathBuf,
    bind: Option<&String>,
) -> String {
    let bind_arg = bind
        .map(|b| format!(" --bind {}", b))
        .unwrap_or_default();

    format!(
        "[Unit]\n\
Description=VelocityDB service\n\
After=network.target\n\
\n\
[Service]\n\
Type=simple\n\
ExecStart={} service run --config {} --data-dir {}{} --verbose\n\
Restart=on-failure\n\
User=velocity\n\
\n\
[Install]\n\
WantedBy=multi-user.target\n",
        exe_path,
        config.display(),
        data_dir.display(),
        bind_arg
    )
}

fn generate_windows_script(exe_path: &str, config: &PathBuf, data_dir: &PathBuf) -> String {
    format!(
        "param(\n    [string]$ServiceName = \"VelocityDB\",\n    [string]$DisplayName = \"VelocityDB Service\"\n)\n\n$binPath = \"{} service run --config {} --data-dir {} --verbose\"\n\nsc.exe create $ServiceName binPath= \"$binPath\" DisplayName= \"$DisplayName\" start= auto\nsc.exe description $ServiceName \"VelocityDB background service\"\n",
        exe_path,
        config.display(),
        data_dir.display(),
    )
}

fn install_templates(
    dir: &PathBuf,
    config: &PathBuf,
    data_dir: &PathBuf,
    bind: Option<&String>,
) -> Result<(), Box<dyn std::error::Error>> {
    fs::create_dir_all(dir)?;
    let exe = std::env::current_exe()?;
    let exe_str = exe.display().to_string();
    let unit = generate_systemd_unit(&exe_str, config, data_dir, bind);
    let script = generate_windows_script(&exe_str, config, data_dir);
    fs::write(dir.join("velocity.service"), unit)?;
    fs::write(dir.join("install-velocity.ps1"), script)?;
    println!(
        "{} Service templates written to {:?}",
        "[SUCCESS]".green(),
        dir
    );
    Ok(())
}

fn uninstall_templates(dir: &PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    if dir.exists() {
        fs::remove_dir_all(dir)?;
        println!("{} Removed templates under {:?}", "[WARN]".yellow(), dir);
    } else {
        println!("{} No templates found at {:?}", "[WARN]".yellow(), dir);
    }
    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = ServiceCli::parse();

    match cli.command {
        ServiceCommand::Run { pid_file } => {
            run_velocity_service(ServiceSpec {
                config_path: cli.config,
                data_dir: cli.data_dir,
                bind: cli.bind,
                verbose: cli.verbose,
                pid_file,
                watch_config: true,
            })
            .await?;
        }
        ServiceCommand::Install {} => {
            install_templates(&cli.template_dir, &cli.config, &cli.data_dir, cli.bind.as_ref())?;
        }
        ServiceCommand::Uninstall {} => {
            uninstall_templates(&cli.template_dir)?;
        }
    }

    Ok(())
}
