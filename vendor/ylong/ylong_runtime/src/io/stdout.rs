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

use std::future::Future;
use std::io;
use std::io::Write;
use std::pin::Pin;
use std::task::{Context, Poll};

use crate::io::{AsyncWrite, State};
use crate::spawn_blocking;

/// A handle to the global standard output stream of the current process.
///
/// `Stdout` implements the [`AsyncWrite`] trait.
pub struct Stdout {
    std: Option<io::Stdout>,
    state: State<io::Stdout>,
    has_written: bool,
}

/// Constructs a new handle to the standard output of the current process.
///
/// # Example
/// ```
/// use ylong_runtime::io::stdout;
/// let _stdout = stdout();
/// ```
pub fn stdout() -> Stdout {
    let std = io::stdout();
    Stdout {
        std: Some(std),
        state: State::init(),
        has_written: false,
    }
}

impl AsyncWrite for Stdout {
    crate::io::stdio::std_async_write!();
}

#[cfg(unix)]
use std::os::unix::io::{AsFd, AsRawFd, BorrowedFd, RawFd};

#[cfg(unix)]
impl AsRawFd for Stdout {
    fn as_raw_fd(&self) -> RawFd {
        io::stdout().as_raw_fd()
    }
}

#[cfg(unix)]
impl AsFd for Stdout {
    fn as_fd(&self) -> BorrowedFd<'_> {
        unsafe { BorrowedFd::borrow_raw(self.as_raw_fd()) }
    }
}

#[cfg(windows)]
use std::os::windows::io::{AsHandle, AsRawHandle, BorrowedHandle, RawHandle};

#[cfg(windows)]
impl AsRawHandle for Stdout {
    fn as_raw_handle(&self) -> RawHandle {
        io::stdout().as_raw_handle()
    }
}

#[cfg(windows)]
impl AsHandle for Stdout {
    fn as_handle(&self) -> BorrowedHandle<'_> {
        unsafe { BorrowedHandle::borrow_raw(self.as_raw_handle()) }
    }
}
