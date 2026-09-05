use std::collections::VecDeque;
use std::path::Path;
use std::sync::Arc;

use parking_lot::Mutex;

use hap_resigner::device::hdc::{CommandOutput, DeviceInfo, HdcClient, HdcRunner};

#[test]
fn lists_installs_launches_and_checks_pid_on_single_target() {
    let runner = Arc::new(FakeRunner::new([
        ok("device-serial\tConnected\n"),
        ok(
            "udid of current device is :\nAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA\n",
        ),
        ok("TYR-AL00\n"),
        ok("install bundle successfully\n"),
        ok("start ability successfully.\n"),
        ok("1234\n"),
    ]));
    let client = HdcClient::with_runner(runner.clone());

    let device = client
        .list_devices()
        .expect("devices")
        .into_iter()
        .next()
        .expect("device");
    assert_eq!(device.target, "device-serial");
    assert_eq!(device.model, "TYR-AL00");
    client.install(&device, Path::new("/tmp/app.hap")).unwrap();
    client
        .launch(&device, "com.example.test", "EntryAbility")
        .unwrap();
    assert_eq!(client.pid(&device, "com.example.test").unwrap(), 1234);

    assert_eq!(
        runner.calls(),
        vec![
            vec!["list", "targets"],
            vec!["-t", "device-serial", "shell", "bm", "get", "--udid"],
            vec![
                "-t",
                "device-serial",
                "shell",
                "param",
                "get",
                "const.product.model"
            ],
            vec!["-t", "device-serial", "install", "-r", "-d", "/tmp/app.hap"],
            vec![
                "-t",
                "device-serial",
                "shell",
                "aa",
                "start",
                "-b",
                "com.example.test",
                "-a",
                "EntryAbility"
            ],
            vec!["-t", "device-serial", "shell", "pidof", "com.example.test"],
        ]
    );
}

#[test]
fn lists_all_targets_in_order_with_targeted_probes() {
    let runner = Arc::new(FakeRunner::new([
        ok("target-beta\tConnected\ntarget-alpha\tConnected\n"),
        ok(
            "udid of current device is :\nBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB\n",
        ),
        ok("Beta Model\n"),
        ok(
            "udid of current device is :\nAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA\n",
        ),
        ok("Alpha Model\n"),
    ]));
    let client = HdcClient::with_runner(runner.clone());

    let devices = client.list_devices().expect("devices");

    assert_eq!(
        devices,
        vec![
            DeviceInfo {
                target: "target-beta".to_owned(),
                udid: "BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB".to_owned(),
                model: "Beta Model".to_owned(),
            },
            DeviceInfo {
                target: "target-alpha".to_owned(),
                udid: "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA".to_owned(),
                model: "Alpha Model".to_owned(),
            },
        ]
    );
    assert_eq!(
        runner.calls(),
        vec![
            vec!["list", "targets"],
            vec!["-t", "target-beta", "shell", "bm", "get", "--udid"],
            vec![
                "-t",
                "target-beta",
                "shell",
                "param",
                "get",
                "const.product.model"
            ],
            vec!["-t", "target-alpha", "shell", "bm", "get", "--udid"],
            vec![
                "-t",
                "target-alpha",
                "shell",
                "param",
                "get",
                "const.product.model"
            ],
        ]
    );
}

#[test]
fn keeps_healthy_devices_when_an_earlier_target_probe_fails() {
    let invalid_udid = "SECRET-FAILED-DEVICE-IDENTIFIER";
    let runner = Arc::new(FakeRunner::new([
        ok("target-broken\tConnected\ntarget-healthy\tConnected\n"),
        ok(invalid_udid),
        ok(
            "udid of current device is :\nCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCC\n",
        ),
        ok("Healthy Model\n"),
    ]));
    let client = HdcClient::with_runner(runner.clone());

    let devices = client.list_devices().expect("healthy device");

    assert_eq!(
        devices,
        vec![DeviceInfo {
            target: "target-healthy".to_owned(),
            udid: "CCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCC".to_owned(),
            model: "Healthy Model".to_owned(),
        }]
    );
    assert_eq!(
        runner.calls(),
        vec![
            vec!["list", "targets"],
            vec!["-t", "target-broken", "shell", "bm", "get", "--udid"],
            vec!["-t", "target-healthy", "shell", "bm", "get", "--udid"],
            vec![
                "-t",
                "target-healthy",
                "shell",
                "param",
                "get",
                "const.product.model"
            ],
        ]
    );
    assert!(!format!("{devices:?}").contains(invalid_udid));
}

#[test]
fn skips_unauthorized_and_offline_target_lines_without_probing_them() {
    let runner = Arc::new(FakeRunner::new([
        ok("target-unauthorized\tUnauthorized\ntarget-offline\tOffline\ntarget-ready\tConnected\n"),
        ok(
            "udid of current device is :\nDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDD\n",
        ),
        ok("Ready Model\n"),
    ]));
    let client = HdcClient::with_runner(runner.clone());

    let devices = client.list_devices().expect("ready device");

    assert_eq!(devices.len(), 1);
    assert_eq!(devices[0].target, "target-ready");
    assert_eq!(
        runner.calls(),
        vec![
            vec!["list", "targets"],
            vec!["-t", "target-ready", "shell", "bm", "get", "--udid"],
            vec![
                "-t",
                "target-ready",
                "shell",
                "param",
                "get",
                "const.product.model"
            ],
        ]
    );
}

#[test]
fn returns_an_empty_device_list_when_hdc_has_no_targets() {
    let runner = Arc::new(FakeRunner::new([ok("[Empty]\n")]));
    let client = HdcClient::with_runner(runner.clone());

    assert!(client.list_devices().expect("devices").is_empty());
    assert_eq!(runner.calls(), vec![vec!["list", "targets"]]);
}

#[test]
fn device_probe_error_names_last_failed_target_without_exposing_invalid_udids() {
    let first_invalid_udid = "SECRET-FIRST-DEVICE-IDENTIFIER";
    let last_invalid_udid = "SECRET-LAST-DEVICE-IDENTIFIER";
    let runner = Arc::new(FakeRunner::new([
        ok("target-first\tConnected\ntarget-last\tConnected\n"),
        ok(first_invalid_udid),
        ok(last_invalid_udid),
    ]));
    let client = HdcClient::with_runner(runner);

    let error = client
        .list_devices()
        .expect_err("all probes must fail")
        .to_string();

    assert!(error.contains("target-last"));
    assert!(!error.contains(first_invalid_udid));
    assert!(!error.contains(last_invalid_udid));
}

#[test]
fn install_failure_preserves_original_case_and_newlines() {
    let failure = "INSTALL_FAILED_VERSION_DOWNGRADE\nPermission denied\n";
    let runner = Arc::new(FakeRunner::new([ok(failure)]));
    let client = HdcClient::with_runner(runner);
    let device = DeviceInfo {
        target: "device-serial".to_owned(),
        udid: "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA".to_owned(),
        model: "TYR-AL00".to_owned(),
    };

    let error = client
        .install(&device, Path::new("/tmp/app.hap"))
        .expect_err("failed install must return its output")
        .to_string();

    assert_eq!(
        error,
        "hdc command failed: INSTALL_FAILED_VERSION_DOWNGRADE\nPermission denied\n"
    );
}

fn ok(stdout: &str) -> CommandOutput {
    CommandOutput {
        success: true,
        stdout: stdout.to_owned(),
        stderr: String::new(),
    }
}

struct FakeRunner {
    outputs: Mutex<VecDeque<CommandOutput>>,
    calls: Mutex<Vec<Vec<String>>>,
}

impl FakeRunner {
    fn new(outputs: impl IntoIterator<Item = CommandOutput>) -> Self {
        Self {
            outputs: Mutex::new(outputs.into_iter().collect()),
            calls: Mutex::new(Vec::new()),
        }
    }

    fn calls(&self) -> Vec<Vec<String>> {
        self.calls.lock().clone()
    }
}

impl HdcRunner for FakeRunner {
    fn run(&self, args: &[&str]) -> Result<CommandOutput, std::io::Error> {
        self.calls
            .lock()
            .push(args.iter().map(|value| (*value).to_owned()).collect());
        Ok(self.outputs.lock().pop_front().expect("fake output"))
    }
}
