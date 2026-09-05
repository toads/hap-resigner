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

use std::ffi::OsStr;
use std::path::Path;

use ylong_runtime::io::{AsyncReadExt, AsyncWriteExt};
use ylong_runtime::process::pty_process::{Pty, PtyCommand};

/// SDV test cases for pty_process basic.
///
/// # Brief
/// 1. Create a `Pty` and a `Command`.
/// 2. Set configs.
/// 3. `spawn()` the child with pts of `Pty`.
#[test]
fn sdv_pty_process_test() {
    let handle = ylong_runtime::spawn(async {
        let mut command = PtyCommand::new("echo");
        assert_eq!(command.get_program(), "echo");

        command.arg("first").args(["second"]);

        let args: Vec<&OsStr> = command.get_args().collect();
        assert_eq!(args, &["first", "second"]);

        command.env("PATH", "/bin");
        let envs: Vec<(&OsStr, Option<&OsStr>)> = command.get_envs().collect();
        assert_eq!(envs, &[(OsStr::new("PATH"), Some(OsStr::new("/bin")))]);

        command.env_remove("PATH");
        let envs: Vec<(&OsStr, Option<&OsStr>)> = command.get_envs().collect();
        assert_eq!(envs, &[(OsStr::new("PATH"), None)]);

        command.env_clear();
        let envs: Vec<(&OsStr, Option<&OsStr>)> = command.get_envs().collect();
        assert!(envs.is_empty());

        let envs = [(OsStr::new("TZ"), OsStr::new("test"))];
        command.envs(envs);
        let envs: Vec<(&OsStr, Option<&OsStr>)> = command.get_envs().collect();
        assert_eq!(envs, &[(OsStr::new("TZ"), Some(OsStr::new("test")))]);

        command.env_clear();
        let envs: Vec<(&OsStr, Option<&OsStr>)> = command.get_envs().collect();
        assert!(envs.is_empty());

        command.current_dir("/bin");
        assert_eq!(command.get_current_dir(), Some(Path::new("/bin")));
    });
    ylong_runtime::block_on(handle).unwrap();
}

/// SDV test cases for pty_process read and write.
///
/// # Brief
/// 1. Create a `Pty` and a `Command`.
/// 2. `spawn()` the child with pts of `Pty`.
/// 3. Write `Pty` with arg.
/// 4. Read `Pty` with correct result.
#[test]
fn sdv_pty_process_read_and_write_test() {
    let arg = "hello world!";
    ylong_runtime::block_on(async {
        let mut pty = Pty::new().unwrap();
        let pts = pty.pts().unwrap();

        let mut command = PtyCommand::new("echo");
        let mut child = command.spawn(&pts).unwrap();

        pty.write_all(arg.as_bytes()).await.unwrap();

        let status = child.wait().await.unwrap();
        assert!(status.success());

        let mut buf = [0; 14];
        pty.read_exact(&mut buf).await.unwrap();
        pty.flush().await.unwrap();
        pty.shutdown().await.unwrap();
        assert_eq!(String::from_utf8_lossy(&buf).replace(['\n', '\r'], ""), arg);
    });
}

/// SDV test cases for pty split.
///
/// # Brief
/// 1. Create a `Pty` and a `Command` with arg.
/// 2. `spawn()` the child with pts of `Pty`.
/// 3. Write read_pty with arg.
/// 4. Read write_pty with correct result.
#[test]
fn sdv_pty_split_test() {
    let arg = "hello world!";
    ylong_runtime::block_on(async {
        let mut pty = Pty::new().unwrap();
        let pts = pty.pts().unwrap();
        let (mut read_pty, mut write_pty) = pty.split();

        let mut command = PtyCommand::new("echo");
        let mut child = command.spawn(&pts).unwrap();

        write_pty.resize(24, 80, 0, 0).expect("resize set fail!");
        write_pty.write_all(arg.as_bytes()).await.unwrap();
        write_pty.flush().await.unwrap();
        write_pty.shutdown().await.unwrap();

        let status = child.wait().await.unwrap();
        assert!(status.success());

        let mut buf = [0; 14];
        read_pty.read_exact(&mut buf).await.unwrap();
        assert_eq!(String::from_utf8_lossy(&buf).replace(['\n', '\r'], ""), arg);
    });
}

/// SDV test cases for pty into_split.
///
/// # Brief
/// 1. Create a `Pty` and a `Command` with arg.
/// 2. `spawn()` the child with pts of `Pty`.
/// 3. Write read_pty with arg.
/// 4. Read write_pty with correct result.
#[test]
fn sdv_pty_into_split_test() {
    let arg = "hello world!";
    ylong_runtime::block_on(async {
        let pty = Pty::new().unwrap();
        let pts = pty.pts().unwrap();
        let (mut read_pty, mut write_pty) = pty.into_split();

        let mut command = PtyCommand::new("echo");
        let mut child = command.spawn(&pts).unwrap();

        write_pty.resize(24, 80, 0, 0).expect("resize set fail!");
        write_pty.write_all(arg.as_bytes()).await.unwrap();
        write_pty.flush().await.unwrap();
        write_pty.shutdown().await.unwrap();

        let status = child.wait().await.unwrap();
        assert!(status.success());

        let mut buf = [0; 14];
        read_pty.read_exact(&mut buf).await.unwrap();
        assert_eq!(String::from_utf8_lossy(&buf).replace(['\n', '\r'], ""), arg);
    });
}

/// SDV test cases for pty unsplit.
///
/// # Brief
/// 1. Create a `Pty` and a `Command` with arg.
/// 2. `unsplit()` read and write.
/// 3. `spawn()` the child with pts of `Pty`.
/// 4. Write pty with arg.
/// 5. Read pty with correct result.
#[test]
fn sdv_pty_unsplit_test() {
    let arg = "hello world!";
    ylong_runtime::block_on(async {
        let pty = Pty::new().unwrap();
        let pts = pty.pts().unwrap();
        let (read_pty, write_pty) = pty.into_split();
        let mut pty = Pty::unsplit(read_pty, write_pty).expect("unsplit fail!");

        let mut command = PtyCommand::new("echo");
        let mut child = command.spawn(&pts).unwrap();

        pty.write_all(arg.as_bytes()).await.unwrap();

        let status = child.wait().await.unwrap();
        assert!(status.success());

        let mut buf = [0; 14];
        pty.read_exact(&mut buf).await.unwrap();
        assert_eq!(String::from_utf8_lossy(&buf).replace(['\n', '\r'], ""), arg);
    });
}

/// SDV test cases for pty debug.
///
/// # Brief
/// 1. Debug pty and splitPty.
/// 2. Check format is correct.
#[test]
fn sdv_pty_debug_test() {
    ylong_runtime::block_on(async {
        let pty = Pty::new().unwrap();
        let pts = pty.pts().unwrap();
        assert!(format!("{pts:?}").contains("Pts(PtsInner(OwnedFd { fd:"));
        let (read_pty, write_pty) = pty.into_split();
        assert!(format!("{read_pty:?}")
            .contains("SplitReadPty(Pty(AsyncSource { io: Some(PtyInner(OwnedFd { fd:"));
        assert!(format!("{write_pty:?}")
            .contains("SplitWritePty(Pty(AsyncSource { io: Some(PtyInner(OwnedFd { fd:"));
        let mut pty = Pty::unsplit(read_pty, write_pty).expect("unsplit fail!");
        let (read_pty, write_pty) = pty.split();
        assert!(format!("{read_pty:?}")
            .contains("BorrowReadPty(Pty(AsyncSource { io: Some(PtyInner(OwnedFd { fd:"));
        assert!(format!("{write_pty:?}")
            .contains("BorrowWritePty(Pty(AsyncSource { io: Some(PtyInner(OwnedFd { fd:"));
    });
}
