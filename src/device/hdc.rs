use std::path::{Path, PathBuf};
use std::sync::Arc;
#[cfg(feature = "inline-hdc")]
use std::thread;
#[cfg(feature = "inline-hdc")]
use std::time::Duration;

use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandOutput {
    pub success: bool,
    pub stdout: String,
    pub stderr: String,
}

pub trait HdcRunner: Send + Sync {
    fn run(&self, args: &[&str]) -> Result<CommandOutput, std::io::Error>;
}

#[cfg(feature = "inline-hdc")]
#[derive(Debug, Default)]
pub struct InlineRunner;

#[cfg(feature = "inline-hdc")]
impl HdcRunner for InlineRunner {
    fn run(&self, args: &[&str]) -> Result<CommandOutput, std::io::Error> {
        let mut last_error = None;
        for _ in 0..5 {
            match hdc_host_inline::run_command(args) {
                Ok(stdout) => {
                    let success = !stdout.contains("[Fail]");
                    return Ok(CommandOutput {
                        success,
                        stdout,
                        stderr: String::new(),
                    });
                }
                Err(error) => {
                    last_error = Some(error);
                    thread::sleep(Duration::from_millis(200));
                }
            }
        }
        Err(last_error.unwrap_or_else(|| std::io::Error::other("inline HDC failed")))
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceInfo {
    pub target: String,
    pub udid: String,
    pub model: String,
}

#[derive(Debug, Error)]
pub enum HdcError {
    #[error("hdc executable was not found")]
    ToolNotFound,
    #[error("hdc I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("failed to probe {property} for device target {target}")]
    DeviceProbe {
        target: String,
        property: &'static str,
    },
    #[error("hdc command failed: {0}")]
    Command(String),
    #[error("path cannot be represented for hdc: {0}")]
    InvalidPath(PathBuf),
    #[error("application process is not running")]
    ProcessNotRunning,
}

#[derive(Clone)]
pub struct HdcClient {
    runner: Arc<dyn HdcRunner>,
}

impl HdcClient {
    pub fn with_runner<R>(runner: Arc<R>) -> Self
    where
        R: HdcRunner + 'static,
    {
        Self { runner }
    }

    pub fn discover() -> Result<Self, HdcError> {
        #[cfg(feature = "inline-hdc")]
        {
            Ok(Self::with_runner(Arc::new(InlineRunner)))
        }
        #[cfg(not(feature = "inline-hdc"))]
        {
            Err(HdcError::ToolNotFound)
        }
    }

    pub fn list_devices(&self) -> Result<Vec<DeviceInfo>, HdcError> {
        let output = self.run(&["list", "targets"])?;
        let targets = parse_available_targets(&output.stdout);
        let mut devices = Vec::with_capacity(targets.len());
        let mut last_error = None;

        for target in targets {
            match self.probe_target(target) {
                Ok(device) => devices.push(device),
                Err(error) => last_error = Some(error),
            }
        }

        if devices.is_empty() {
            if let Some(error) = last_error {
                return Err(error);
            }
        }
        Ok(devices)
    }

    fn probe_target(&self, target: &str) -> Result<DeviceInfo, HdcError> {
        let probe_error = |property| HdcError::DeviceProbe {
            target: target.to_owned(),
            property,
        };
        let udid_output = self
            .run_target(target, &["shell", "bm", "get", "--udid"])
            .map_err(|_| probe_error("UDID"))?;
        let udid = parse_udid(&udid_output.stdout).ok_or_else(|| probe_error("UDID"))?;
        let model = self
            .run_target(target, &["shell", "param", "get", "const.product.model"])
            .map_err(|_| probe_error("model"))?
            .stdout
            .trim()
            .to_owned();
        Ok(DeviceInfo {
            target: target.to_owned(),
            udid,
            model,
        })
    }

    pub fn install(&self, device: &DeviceInfo, hap: &Path) -> Result<(), HdcError> {
        let path = hap
            .to_str()
            .ok_or_else(|| HdcError::InvalidPath(hap.to_path_buf()))?;
        let output = self.run_target(&device.target, &["install", "-r", "-d", path])?;
        let text = format!("{}{}", output.stdout, output.stderr);
        let succeeded = text.to_ascii_lowercase().contains("success");
        if !succeeded {
            return Err(HdcError::Command(text));
        }
        Ok(())
    }

    pub fn launch(
        &self,
        device: &DeviceInfo,
        bundle_name: &str,
        ability: &str,
    ) -> Result<(), HdcError> {
        self.run_target(
            &device.target,
            &["shell", "aa", "start", "-b", bundle_name, "-a", ability],
        )?;
        Ok(())
    }

    pub fn pid(&self, device: &DeviceInfo, bundle_name: &str) -> Result<u32, HdcError> {
        let output = self.run_target(&device.target, &["shell", "pidof", bundle_name])?;
        output
            .stdout
            .split_whitespace()
            .next()
            .and_then(|value| value.parse().ok())
            .ok_or(HdcError::ProcessNotRunning)
    }

    fn run_target(&self, target: &str, args: &[&str]) -> Result<CommandOutput, HdcError> {
        let mut command = Vec::with_capacity(args.len() + 2);
        command.extend_from_slice(&["-t", target]);
        command.extend_from_slice(args);
        self.run(&command)
    }

    fn run(&self, args: &[&str]) -> Result<CommandOutput, HdcError> {
        let output = self.runner.run(args)?;
        if !output.success {
            return Err(HdcError::Command(format!(
                "{}{}",
                output.stdout, output.stderr
            )));
        }
        Ok(output)
    }
}

fn parse_available_targets(output: &str) -> Vec<&str> {
    output
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            let target = fields.next()?;
            let unavailable = target == "[Empty]"
                || fields.any(|field| {
                    field.eq_ignore_ascii_case("Unauthorized")
                        || field.eq_ignore_ascii_case("Offline")
                });
            (!unavailable).then_some(target)
        })
        .collect()
}

fn parse_udid(output: &str) -> Option<String> {
    output
        .split(|character: char| character.is_whitespace() || character == ':')
        .map(str::trim)
        .find(|value| value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_alphanumeric()))
        .map(str::to_owned)
}
