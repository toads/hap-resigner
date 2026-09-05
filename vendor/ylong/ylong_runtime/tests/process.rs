// Copyright (c) 2023 Huawei Device Co., Ltd.
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

#![cfg(all(target_os = "linux", feature = "process"))]

use std::os::fd::{AsFd, AsRawFd};
use std::process::Stdio;

use ylong_runtime::io::{AsyncReadExt, AsyncWriteExt};
use ylong_runtime::process::{ChildStderr, ChildStdin, ChildStdout, Command};

/// SDV test cases for `output()`.
///
/// # Brief
/// 1. Create a `Command` with arg.
/// 2. Use `output()` waiting result.
#[test]
fn sdv_process_output_test() {
    let handle = ylong_runtime::spawn(async {
        let mut command = Command::new("echo");
        command.arg("Hello, world!");
        let output = command.output().await.unwrap();

        assert!(output.status.success());
        assert_eq!(output.stdout.as_slice(), b"Hello, world!\n");
        assert!(output.stderr.is_empty());
    });
    ylong_runtime::block_on(handle).unwrap();
}

/// SDV test cases for `status()`.
///
/// # Brief
/// 1. Create a `Command` with arg.
/// 2. Use `status()` waiting result.
#[test]
fn sdv_process_status_test() {
    let handle = ylong_runtime::spawn(async {
        let mut command = Command::new("echo");
        command.arg("Hello, world!");

        let status = command.status().await.unwrap();
        assert!(status.success());
    });
    ylong_runtime::block_on(handle).unwrap();
}

/// SDV test cases for `spawn()`.
///
/// # Brief
/// 1. Create a `Command` with arg.
/// 2. Use `spawn()` create a child handle
/// 3. Use `wait()` waiting result.
#[test]
fn sdv_process_spawn_test() {
    let handle = ylong_runtime::spawn(async {
        let mut command = Command::new("echo");
        command.arg("Hello, world!");
        let mut child = command.spawn().unwrap();
        assert!(child.id().is_some());

        let status = child.wait().await.unwrap();
        assert!(status.success());
        assert!(child.start_kill().is_err());
        assert!(child.id().is_none());

        let status = child.wait().await.unwrap();
        assert!(status.success());
    });
    ylong_runtime::block_on(handle).unwrap();
}

/// SDV test cases for ChildStdio.
///
/// # Brief
/// 1. Create a `Command` and `spawn()`.
/// 2. Take `child.stdin` and write something in it.
/// 3. Take `child.stdout` and read it, check the result.
/// 4. Check child's result.
#[test]
fn sdv_process_child_stdio_test() {
    let handle = ylong_runtime::spawn(async {
        let mut child = Command::new("rev")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("Failed to spawn child process");

        let mut stdin = child.take_stdin().expect("Failed to open stdin");
        let stdin_handle = ylong_runtime::spawn(async move {
            stdin.write_all(b"Hello, world!").await.unwrap();
            stdin.flush().await.unwrap();
            stdin.shutdown().await.unwrap();
        });

        let mut stdout = child.take_stdout().expect("Failed to open stdout");
        let stdout_handle = ylong_runtime::spawn(async move {
            let mut buf = Vec::new();
            stdout.read_to_end(&mut buf).await.unwrap();
            let str = "!dlrow ,olleH";
            assert!(String::from_utf8(buf).unwrap().contains(str));
        });

        let mut stderr = child.take_stderr().expect("Failed to open stderr");
        let stderr_handle = ylong_runtime::spawn(async move {
            let mut buf = Vec::new();
            stderr.read_to_end(&mut buf).await.unwrap();
            assert!(buf.is_empty());
        });

        let status = child.wait().await.unwrap();
        assert!(status.success());

        stdin_handle.await.unwrap();
        stdout_handle.await.unwrap();
        stderr_handle.await.unwrap();
    });
    ylong_runtime::block_on(handle).unwrap();
}

/// SDV test cases for `kill()`.
///
/// # Brief
/// 1. Create a `Command` with arg.
/// 2. Use `spawn()` create a child handle
/// 3. Use `kill()` to kill the child handle.
#[test]
fn sdv_process_kill_test() {
    let handle = ylong_runtime::spawn(async {
        let mut command = Command::new("echo");
        command.arg("Hello, world!");
        let mut child = command.spawn().unwrap();

        assert!(child.kill().await.is_ok());
    });
    ylong_runtime::block_on(handle).unwrap();
}

/// SDV test cases for `try_wait()`.
///
/// # Brief
/// 1. Create a `Command` with arg.
/// 2. Use `spawn()` create a child handle
/// 3. Use `try_wait()` waiting result until the child handle is ok.
#[test]
fn sdv_process_try_wait_test() {
    let handle = ylong_runtime::spawn(async {
        let mut command = Command::new("echo");
        command.arg("Hello, world!");
        let mut child = command.spawn().unwrap();

        loop {
            if child.try_wait().unwrap().is_some() {
                break;
            }
        }
        assert!(child.try_wait().unwrap().is_some());
    });
    ylong_runtime::block_on(handle).unwrap();
}

/// SDV test cases for drop.
///
/// # Brief
/// 1. Create a `Command` with kill_on_drop.
/// 2. Use `spawn()` create a child handle
/// 3. Use `drop()` to drop the child handle.
#[test]
fn sdv_process_drop_test() {
    let handle = ylong_runtime::spawn(async {
        let mut command = Command::new("echo");
        command.arg("Hello, world!").kill_on_drop(true);
        let child = command.spawn();
        assert!(child.is_ok());
        drop(child.unwrap());

        let mut command = Command::new("echo");
        command.arg("Hello, world!");
        let child = command.spawn();
        assert!(child.is_ok());
        drop(child.unwrap());
    });
    ylong_runtime::block_on(handle).unwrap();
}

/// SDV test cases for stdio.
///
/// # Brief
/// 1. Create a `Command` with arg.
/// 2. Use `spawn()` create a child handle
/// 3. Use `wait()` waiting result.
#[test]
fn sdv_process_stdio_test() {
    let handle = ylong_runtime::spawn(async {
        let mut command = Command::new("echo");
        command
            .arg("Hello, world!")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = command.spawn().unwrap();

        let child_stdin = child.take_stdin().unwrap();
        assert!(child_stdin.into_owned_fd().is_ok());
        let child_stdout = child.take_stdout().unwrap();
        assert!(child_stdout.into_owned_fd().is_ok());
        let child_stderr = child.take_stderr().unwrap();
        assert!(child_stderr.into_owned_fd().is_ok());

        drop(child);

        let mut child = command.spawn().unwrap();

        let child_stdin = child.take_stdin().unwrap();
        assert!(child_stdin.as_fd().as_raw_fd() >= 0);
        assert!(child_stdin.as_raw_fd() >= 0);
        assert!(TryInto::<Stdio>::try_into(child_stdin).is_ok());

        let child_stdout = child.take_stdout().unwrap();
        assert!(child_stdout.as_fd().as_raw_fd() >= 0);
        assert!(child_stdout.as_raw_fd() >= 0);
        assert!(TryInto::<Stdio>::try_into(child_stdout).is_ok());

        let child_stderr = child.take_stderr().unwrap();
        assert!(child_stderr.as_fd().as_raw_fd() >= 0);
        assert!(child_stderr.as_raw_fd() >= 0);
        assert!(TryInto::<Stdio>::try_into(child_stderr).is_ok());
        drop(child);
    });
    ylong_runtime::block_on(handle).unwrap();
}

/// SDV test cases for ChildStd.
///
/// # Brief
/// 1. Create a `std::process::Command`.
/// 2. Use `spawn()` create a child handle
/// 3. Use `from_std()` to convert std to ylong_runtime::process::ChildStd.
#[test]
fn sdv_process_child_stdio_convert_test() {
    let handle = ylong_runtime::spawn(async {
        let mut command = std::process::Command::new("echo");
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = command.spawn().unwrap();
        let stdin = child.stdin.take().unwrap();
        assert!(ChildStdin::from_std(stdin).is_ok());
        let stdout = child.stdout.take().unwrap();
        assert!(ChildStdout::from_std(stdout).is_ok());
        let stderr = child.stderr.take().unwrap();
        assert!(ChildStderr::from_std(stderr).is_ok());
    });
    ylong_runtime::block_on(handle).unwrap();
}

/// SDV test cases for command debug.
///
/// # Brief
/// 1. Debug Command and Child.
/// 2. Check format is correct.
#[test]
fn sdv_process_debug_test() {
    let handle = ylong_runtime::spawn(async {
        let mut command = Command::new("echo");
        assert_eq!(
            format!("{command:?}"),
            "Command { std: \"echo\", kill: false }"
        );
        let mut child = command.spawn().unwrap();

        assert_eq!(format!("{child:?}"), "Child { state: Pending(Some(Child { stdin: None, stdout: None, stderr: None, .. })), kill_on_drop: false, stdin: None, stdout: None, stderr: None }");
        let status = child.wait().await.unwrap();
        assert!(status.success());
    });
    ylong_runtime::block_on(handle).unwrap();
}
