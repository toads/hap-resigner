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

//! This test can only run in cargo.

#[cfg(unix)]
use std::os::fd::{AsFd, AsRawFd};

use ylong_runtime::io::{AsyncBufWriter, AsyncWriteExt};

/// UT test cases for `stdout` and `stderr``.
///
/// # Brief
/// 1. create a `stdout` and a `stderr`.
/// 2. write something into `stdout` and `stderr`.
/// 3. check operation is ok.
#[test]
fn sdv_stdio_write() {
    ylong_runtime::block_on(async {
        let mut stdout = ylong_runtime::io::stdout();
        #[cfg(unix)]
        assert!(stdout.as_fd().as_raw_fd() >= 0);
        #[cfg(unix)]
        assert!(stdout.as_raw_fd() >= 0);
        let res = stdout.write_all(b"something").await;
        assert!(res.is_ok());
        let res = stdout.flush().await;
        assert!(res.is_ok());
        let res = stdout.shutdown().await;
        assert!(res.is_ok());

        let mut stderr = ylong_runtime::io::stderr();
        #[cfg(unix)]
        assert!(stderr.as_fd().as_raw_fd() >= 0);
        #[cfg(unix)]
        assert!(stderr.as_raw_fd() >= 0);
        let res = stderr.write_all(b"something").await;
        assert!(res.is_ok());
        let res = stderr.flush().await;
        assert!(res.is_ok());
        let res = stderr.shutdown().await;
        assert!(res.is_ok());
    });
}

/// SDV test cases for `stdout` and `stderr``.
///
/// # Brief
/// 1. create a `stdout` and a `stderr`.
/// 2. write something into `stdout` and `stderr`.
/// 3. check operation is ok.
#[test]
fn sdv_stdio_buf_writer_write() {
    let handle = ylong_runtime::spawn(async {
        let stdout = ylong_runtime::io::stdout();
        let mut buf_writer = AsyncBufWriter::new(stdout);
        let res = buf_writer.write_all(b"something").await;
        assert!(res.is_ok());
        let res = buf_writer.flush().await;
        assert!(res.is_ok());
        let res = buf_writer.shutdown().await;
        assert!(res.is_ok());

        let stderr = ylong_runtime::io::stderr();
        let mut buf_writer = AsyncBufWriter::new(stderr);
        let res = buf_writer.write_all(b"something").await;
        assert!(res.is_ok());
        let res = buf_writer.flush().await;
        assert!(res.is_ok());
        let res = buf_writer.shutdown().await;
        assert!(res.is_ok());
    });
    ylong_runtime::block_on(handle).unwrap();
}
