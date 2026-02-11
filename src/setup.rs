use colored::*;
use std::fs;
use std::path::{Path, PathBuf};

pub struct SetupInstallSpec {
    pub config: PathBuf,
    pub data_dir: PathBuf,
    pub bind: Option<String>,
    pub bin_dir: Option<PathBuf>,
    pub service_file: Option<PathBuf>,
    pub with_service: bool,
}

pub fn default_bin_dir() -> PathBuf {
    if cfg!(target_os = "windows") {
        let base = std::env::var_os("ProgramFiles")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(r"C:\Program Files"));
        return base.join("Velocity").join("bin");
    }

    if cfg!(target_os = "linux") {
        return PathBuf::from("/opt/velocity/bin");
    }

    PathBuf::from("/usr/local/bin")
}

pub fn default_service_file() -> PathBuf {
    if cfg!(target_os = "windows") {
        let base = std::env::var_os("ProgramFiles")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(r"C:\Program Files"));
        return base
            .join("Velocity")
            .join("service")
            .join("install-velocity.ps1");
    }

    if cfg!(target_os = "linux") {
        return PathBuf::from("/etc/systemd/system/velocity.service");
    }

    PathBuf::from("./velocity.service")
}

pub fn print_default_paths() {
    println!(
        "{} Binary install directory: {}",
        "[SETUP]".blue(),
        default_bin_dir().display()
    );
    println!(
        "{} Service file/script: {}",
        "[SETUP]".blue(),
        default_service_file().display()
    );
}

pub fn run_setup_install(spec: SetupInstallSpec) -> Result<(), Box<dyn std::error::Error>> {
    let exe = std::env::current_exe()?;
    let exe_name = if cfg!(target_os = "windows") {
        "velocity.exe"
    } else {
        "velocity"
    };

    let bin_dir = spec.bin_dir.unwrap_or_else(default_bin_dir);
    ensure_dir(&bin_dir)?;

    let installed_exe = bin_dir.join(exe_name);
    fs::copy(&exe, &installed_exe)?;
    println!(
        "{} Installed executable to {}",
        "[SUCCESS]".green(),
        installed_exe.display()
    );

    // Install helper binary when available (built from src/bin/service.rs).
    let helper_name = if cfg!(target_os = "windows") {
        "service.exe"
    } else {
        "service"
    };
    if let Some(sibling) = exe.parent().map(|p| p.join(helper_name)) {
        if sibling.exists() {
            let helper_dst = bin_dir.join(helper_name);
            fs::copy(&sibling, &helper_dst)?;
            println!(
                "{} Installed helper binary to {}",
                "[SUCCESS]".green(),
                helper_dst.display()
            );
        }
    }

    if spec.with_service {
        let service_file = spec.service_file.unwrap_or_else(default_service_file);
        if let Some(parent) = service_file.parent() {
            ensure_dir(parent)?;
        }

        if cfg!(target_os = "windows") {
            fs::write(
                &service_file,
                render_windows_service_installer(
                    &installed_exe,
                    &spec.config,
                    &spec.data_dir,
                    spec.bind.as_deref(),
                ),
            )?;
            println!(
                "{} Wrote service installer script to {}",
                "[SUCCESS]".green(),
                service_file.display()
            );
            println!(
                "{} Run as administrator: powershell -ExecutionPolicy Bypass -File \"{}\"",
                "[NEXT]".yellow(),
                service_file.display()
            );
        } else {
            fs::write(
                &service_file,
                render_systemd_unit(
                    &installed_exe,
                    &spec.config,
                    &spec.data_dir,
                    spec.bind.as_deref(),
                ),
            )?;
            println!(
                "{} Wrote systemd unit to {}",
                "[SUCCESS]".green(),
                service_file.display()
            );
            println!(
                "{} Run: sudo systemctl daemon-reload && sudo systemctl enable --now velocity",
                "[NEXT]".yellow()
            );
        }
    }

    Ok(())
}

fn ensure_dir(path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    fs::create_dir_all(path).map_err(|e| {
        if e.kind() == std::io::ErrorKind::PermissionDenied {
            return format!(
                "Permission denied creating {}. Re-run with administrator/root privileges.",
                path.display()
            )
            .into();
        }
        e.into()
    })
}

fn render_systemd_unit(
    installed_exe: &Path,
    config: &Path,
    data_dir: &Path,
    bind: Option<&str>,
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
ExecStart={} ops service run --config {} --data-dir {}{} --verbose\n\
Restart=on-failure\n\
\n\
[Install]\n\
WantedBy=multi-user.target\n",
        installed_exe.display(),
        config.display(),
        data_dir.display(),
        bind_arg
    )
}

fn render_windows_service_installer(
    installed_exe: &Path,
    config: &Path,
    data_dir: &Path,
    bind: Option<&str>,
) -> String {
    let bind_arg = bind
        .map(|b| format!(" --bind {}", b))
        .unwrap_or_default();
    format!(
        "param(\n    [string]$ServiceName = \"VelocityDB\",\n    [string]$DisplayName = \"VelocityDB Service\"\n)\n\n\
$binPath = '\"{}\" ops service run --config \"{}\" --data-dir \"{}\"{} --verbose'\n\n\
sc.exe create $ServiceName binPath= \"$binPath\" DisplayName= \"$DisplayName\" start= auto\n\
sc.exe description $ServiceName \"VelocityDB background service\"\n\
sc.exe start $ServiceName\n",
        installed_exe.display(),
        config.display(),
        data_dir.display(),
        bind_arg
    )
}
