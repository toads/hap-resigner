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
use std::pin::Pin;
use std::task::{Context, Poll};

use crate::io::{AsyncRead, ReadBuf, State};
use crate::spawn_blocking;

/// A handle to the standard input stream of a process.
///
/// `Stdin` implements the [`AsyncRead`] trait.
pub struct Stdin {
    std: Option<io::Stdin>,
    state: State<io::Stdin>,
}

/// Constructs a new handle to the standard input of the current process.
///
/// # Example
/// ```
/// use ylong_runtime::io::stdin;
/// let _stdin = stdin();
/// ```
pub fn stdin() -> Stdin {
    let stdin = io::stdin();
    Stdin {
        std: Some(stdin),
        state: State::init(),
    }
}

impl AsyncRead for Stdin {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        loop {
            match self.state {
                State::Idle(ref mut buf_op) => {
                    // before each take, BufInner is set to some
                    let mut buf_inner = buf_op.take().unwrap();

                    if !buf_inner.is_empty() {
                        buf_inner.clone_into(buf);
                        *buf_op = Some(buf_inner);
                        return Poll::Ready(Ok(()));
                    }

                    buf_inner.set_len(buf);
                    // before each take, std is set to some
                    let mut std = self.std.take().unwrap();
                    let handle = spawn_blocking(move || {
                        let res = buf_inner.read_from(&mut std);
                        (res, buf_inner, std)
                    });

                    self.state = State::Poll(handle);
                }
                State::Poll(ref mut join_handle) => {
                    let (res, mut buf_inner, std) = match Pin::new(join_handle).poll(cx)? {
                        Poll::Ready(t) => t,
                        Poll::Pending => return Poll::Pending,
                    };
                    self.std = Some(std);

                    return match res {
                        Ok(_) => {
                            buf_inner.clone_into(buf);
                            self.state = State::Idle(Some(buf_inner));
                            Poll::Ready(Ok(()))
                        }
                        Err(e) => {
                            self.state = State::Idle(Some(buf_inner));
                            Poll::Ready(Err(e))
                        }
                    };
                }
            }
        }
    }
}

#[cfg(unix)]
use std::os::unix::io::{AsFd, AsRawFd, BorrowedFd, RawFd};

#[cfg(unix)]
impl AsRawFd for Stdin {
    fn as_raw_fd(&self) -> RawFd {
        io::stdin().as_raw_fd()
    }
}

#[cfg(unix)]
impl AsFd for Stdin {
    fn as_fd(&self) -> BorrowedFd<'_> {
        unsafe { BorrowedFd::borrow_raw(self.as_raw_fd()) }
    }
}

#[cfg(windows)]
use std::os::windows::io::{AsHandle, AsRawHandle, BorrowedHandle, RawHandle};

#[cfg(windows)]
impl AsRawHandle for Stdin {
    fn as_raw_handle(&self) -> RawHandle {
        io::stdin().as_raw_handle()
    }
}

#[cfg(windows)]
impl AsHandle for Stdin {
    fn as_handle(&self) -> BorrowedHandle<'_> {
        unsafe { BorrowedHandle::borrow_raw(self.as_raw_handle()) }
    }
}
