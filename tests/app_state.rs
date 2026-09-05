use std::path::PathBuf;

use hap_resigner::app::state::{AppPhase, AppState, LogLevel, WorkflowEvent};
use hap_resigner::device::hdc::DeviceInfo;

#[test]
fn transitions_from_idle_to_signed_and_installable() {
    let mut state = AppState::default();
    assert!(state.can_start());
    assert!(!state.can_install());

    state.apply(WorkflowEvent::Phase(AppPhase::Inspecting));
    state.apply(WorkflowEvent::Progress {
        level: LogLevel::Info,
        message: "Reading HAP".to_owned(),
    });
    state.apply(WorkflowEvent::Signed {
        output: PathBuf::from("/tmp/app-resigned.hap"),
        bundle_name: "com.example.test".to_owned(),
        ability: "EntryAbility".to_owned(),
    });

    assert_eq!(state.phase, AppPhase::ReadyToInstall);
    assert!(state.can_install());
    assert_eq!(state.logs.len(), 1);
}

#[test]
fn waiting_for_login_is_a_reachable_phase() {
    let mut state = AppState::default();

    state.apply(WorkflowEvent::Phase(AppPhase::Authenticating));
    state.apply(WorkflowEvent::Phase(AppPhase::WaitingForLogin));

    assert_eq!(state.phase, AppPhase::WaitingForLogin);
}

#[test]
fn devices_event_replaces_the_complete_device_list() {
    let mut state = AppState::default();
    let first_scan = vec![device("target-1", "udid-1", "model-1")];
    let second_scan = vec![
        device("target-2", "udid-2", "model-2"),
        device("target-3", "udid-3", "model-3"),
    ];

    state.apply(WorkflowEvent::Devices(first_scan));
    state.apply(WorkflowEvent::Devices(second_scan.clone()));

    assert_eq!(state.devices, second_scan);
}

#[test]
fn device_scan_failure_is_non_terminal() {
    let mut state = AppState::default();
    state.apply(WorkflowEvent::Phase(AppPhase::Signing));

    state.apply(WorkflowEvent::DeviceScanFailed(
        "hdc unavailable".to_owned(),
    ));

    assert_eq!(state.phase, AppPhase::Signing);
    assert_eq!(state.device_scan_error.as_deref(), Some("hdc unavailable"));
    assert_eq!(state.error, None);
    assert_eq!(
        state.logs.last().map(|entry| entry.level),
        Some(LogLevel::Warning)
    );
}

#[test]
fn failure_records_the_phase_that_failed() {
    let mut state = AppState::default();
    state.apply(WorkflowEvent::Phase(AppPhase::PreparingMaterials));
    state.apply(WorkflowEvent::Failed("network failed".to_owned()));

    assert_eq!(state.phase, AppPhase::Error);
    assert_eq!(state.failed_phase, Some(AppPhase::PreparingMaterials));
    assert_eq!(state.error.as_deref(), Some("network failed"));
    assert!(state.can_start());
}

fn device(target: &str, udid: &str, model: &str) -> DeviceInfo {
    DeviceInfo {
        target: target.to_owned(),
        udid: udid.to_owned(),
        model: model.to_owned(),
    }
}
