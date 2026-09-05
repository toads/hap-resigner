use std::path::PathBuf;

use crate::device::hdc::DeviceInfo;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum AppPhase {
    #[default]
    Idle,
    Inspecting,
    Authenticating,
    WaitingForLogin,
    PreparingMaterials,
    Signing,
    ReadyToInstall,
    Installing,
    Done,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogLevel {
    Info,
    Success,
    Warning,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogEntry {
    pub level: LogLevel,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkflowEvent {
    Phase(AppPhase),
    Progress {
        level: LogLevel,
        message: String,
    },
    Devices(Vec<DeviceInfo>),
    DeviceScanFailed(String),
    Signed {
        output: PathBuf,
        bundle_name: String,
        ability: String,
    },
    Installed {
        pid: u32,
    },
    Failed(String),
}

#[derive(Debug, Default)]
pub struct AppState {
    pub phase: AppPhase,
    pub failed_phase: Option<AppPhase>,
    pub logs: Vec<LogEntry>,
    pub error: Option<String>,
    pub device_scan_error: Option<String>,
    pub devices: Vec<DeviceInfo>,
    pub device: Option<DeviceInfo>,
    pub signed_hap: Option<PathBuf>,
    pub bundle_name: Option<String>,
    pub ability: Option<String>,
    pub pid: Option<u32>,
}

impl AppState {
    pub fn can_start(&self) -> bool {
        matches!(
            self.phase,
            AppPhase::Idle | AppPhase::ReadyToInstall | AppPhase::Done | AppPhase::Error
        )
    }

    pub fn can_install(&self) -> bool {
        self.phase == AppPhase::ReadyToInstall && self.signed_hap.is_some()
    }

    pub fn apply(&mut self, event: WorkflowEvent) {
        match event {
            WorkflowEvent::Phase(phase) => {
                self.phase = phase;
                if phase == AppPhase::Inspecting {
                    self.error = None;
                    self.failed_phase = None;
                    self.pid = None;
                }
            }
            WorkflowEvent::Progress { level, message } => {
                self.logs.push(LogEntry { level, message });
            }
            WorkflowEvent::Devices(devices) => {
                self.devices = devices;
                self.device_scan_error = None;
            }
            WorkflowEvent::DeviceScanFailed(error) => {
                self.logs.push(LogEntry {
                    level: LogLevel::Warning,
                    message: error.clone(),
                });
                self.device_scan_error = Some(error);
            }
            WorkflowEvent::Signed {
                output,
                bundle_name,
                ability,
            } => {
                self.signed_hap = Some(output);
                self.bundle_name = Some(bundle_name);
                self.ability = Some(ability);
                self.phase = AppPhase::ReadyToInstall;
            }
            WorkflowEvent::Installed { pid } => {
                self.pid = Some(pid);
                self.phase = AppPhase::Done;
            }
            WorkflowEvent::Failed(error) => {
                self.failed_phase = Some(self.phase);
                self.logs.push(LogEntry {
                    level: LogLevel::Error,
                    message: error.clone(),
                });
                self.error = Some(error);
                self.phase = AppPhase::Error;
            }
        }
    }
}
