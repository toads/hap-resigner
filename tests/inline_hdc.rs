#![cfg(feature = "inline-hdc")]

use std::env;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use hap_resigner::app::state::WorkflowEvent;
use hap_resigner::app::workflow::{spawn_device_scan, spawn_install, spawn_resign};
use hap_resigner::device::hdc::HdcClient;

#[test]
#[ignore = "requires a connected and authorized HarmonyOS device"]
fn inline_hdc_lists_at_least_one_device() {
    let hdc = HdcClient::discover().expect("inline HDC");
    let devices = hdc.list_devices().expect("device enumeration");
    assert!(!devices.is_empty(), "connected device");
    assert!(devices.iter().all(|device| device.udid.len() == 64));
    let device = &devices[0];

    if let Ok(hap) = env::var("INLINE_HDC_HAP") {
        hdc.install(device, hap.as_ref()).expect("install HAP");
        let bundle = env::var("INLINE_HDC_BUNDLE").expect("INLINE_HDC_BUNDLE");
        let ability = env::var("INLINE_HDC_ABILITY").unwrap_or_else(|_| "EntryAbility".to_owned());
        hdc.launch(device, &bundle, &ability)
            .expect("launch Ability");
        assert!(hdc.pid(device, &bundle).expect("running PID") > 0);
    }
}

#[test]
#[ignore = "requires a connected and authorized HarmonyOS device"]
fn workflow_device_scan_emits_the_complete_list() {
    let expected = HdcClient::discover()
        .expect("inline HDC")
        .list_devices()
        .expect("device enumeration");
    assert!(!expected.is_empty(), "connected device");
    let (events_tx, events_rx) = mpsc::channel();

    spawn_device_scan(events_tx);

    match events_rx
        .recv_timeout(Duration::from_secs(10))
        .expect("device scan event")
    {
        WorkflowEvent::Devices(devices) => assert_eq!(devices, expected),
        WorkflowEvent::DeviceScanFailed(error) => panic!("device scan failed: {error}"),
        event => panic!("unexpected device scan event: {event:?}"),
    }
}

#[test]
#[ignore = "requires live AGC credentials, a connected device, and LIVE_WORKFLOW_HAP"]
fn live_workflow_prepares_signs_installs_and_launches() {
    let hap = env::var("LIVE_WORKFLOW_HAP").expect("LIVE_WORKFLOW_HAP");
    let device = HdcClient::discover()
        .expect("inline HDC")
        .list_devices()
        .expect("device enumeration")
        .into_iter()
        .next()
        .expect("connected device");
    let (events_tx, events_rx) = mpsc::channel();
    spawn_resign(hap.into(), device.clone(), events_tx.clone());
    let deadline = Instant::now() + Duration::from_secs(600);
    let mut install_started = false;

    while Instant::now() < deadline {
        let Ok(event) = events_rx.recv_timeout(Duration::from_secs(5)) else {
            continue;
        };
        match event {
            WorkflowEvent::Phase(phase) => eprintln!("LIVE_PHASE: {phase:?}"),
            WorkflowEvent::Progress { level, message } => {
                eprintln!("LIVE_PROGRESS: {level:?}: {message}")
            }
            WorkflowEvent::Devices(devices) => {
                eprintln!("LIVE_DEVICES: {}", devices.len())
            }
            WorkflowEvent::DeviceScanFailed(error) => {
                eprintln!("LIVE_DEVICE_SCAN_FAILED: {error}")
            }
            WorkflowEvent::Signed {
                output,
                bundle_name,
                ability,
            } => {
                assert!(!install_started, "install triggered more than once");
                eprintln!("LIVE_SIGNED: {}", output.display());
                install_started = true;
                spawn_install(
                    output,
                    bundle_name,
                    ability,
                    device.clone(),
                    events_tx.clone(),
                );
            }
            WorkflowEvent::Installed { pid } => {
                assert!(pid > 0, "application PID must be positive");
                eprintln!("LIVE_INSTALLED: pid={pid}");
                return;
            }
            WorkflowEvent::Failed(error) => panic!("live workflow failed: {error}"),
        }
    }
    panic!("live workflow timed out");
}
