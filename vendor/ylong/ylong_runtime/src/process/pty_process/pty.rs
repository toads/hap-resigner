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

use std::fmt::Debug;
use std::io;
use std::os::fd::{AsFd, AsRawFd, BorrowedFd, OwnedFd, RawFd};
use std::pin::Pin;
use std::process::Stdio;
use std::sync::Arc;
use std::task::{Context, Poll};

use crate::io::{AsyncRead, AsyncWrite, ReadBuf};
use crate::net::AsyncSource;

/// Async `Pty` which implement `AsyncRead` and `AsyncWrite`
#[derive(Debug)]
pub struct Pty(AsyncSource<super::sys::PtyInner>);

impl Pty {
    /// Creates a new async `Pty`
    ///
    /// # Example
    ///
    /// ```no_run
    /// use ylong_runtime::process::pty_process::Pty;
    ///
    /// let _pty = Pty::new().expect("create Pty fail!");
    /// ```
    pub fn new() -> io::Result<Self> {
        let pty = super::sys::PtyInner::open()?;
        pty.set_nonblocking()?;
        let source = AsyncSource::new(pty, None)?;
        Ok(Pty(source))
    }

    /// Changes the size of the terminal with `Pty`
    ///
    /// # Example
    ///
    /// ```no_run
    /// use ylong_runtime::process::pty_process::Pty;
    ///
    /// let pty = Pty::new().expect("create Pty fail!");
    /// pty.resize(24, 80, 0, 0).expect("resize set fail!");
    /// ```
    pub fn resize(
        &self,
        ws_row: u16,
        ws_col: u16,
        ws_xpixel: u16,
        ws_ypixel: u16,
    ) -> io::Result<()> {
        (*self.0).set_size(ws_row, ws_col, ws_xpixel, ws_ypixel)
    }

    /// Open a fd for the other end of the `Pty`, which should be attached to
    /// the child process running in it.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use ylong_runtime::process::pty_process::Pty;
    ///
    /// let pty = Pty::new().expect("create Pty fail!");
    /// let pts = pty.pts().expect("get pts fail!");
    /// ```
    pub fn pts(&self) -> io::Result<Pts> {
        const PATH_BUF_SIZE: usize = 256;
        (*self.0).pts(PATH_BUF_SIZE).map(Pts::new)
    }

    /// Splits a `Pty` into a read half and a write half with reference,
    /// which can be used to read and write the stream concurrently.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use std::io;
    ///
    /// use ylong_runtime::process::pty_process::Pty;
    ///
    /// async fn io_func() -> io::Result<()> {
    ///     let mut pty = Pty::new().expect("create Pty fail!");
    ///     let (read_pty, write_pty) = pty.split();
    ///     Ok(())
    /// }
    /// ```
    pub fn split(&mut self) -> (BorrowReadPty, BorrowWritePty) {
        let read = BorrowReadPty(self);
        let write = BorrowWritePty(self);
        (read, write)
    }

    /// Splits a `Pty` into a read half and a write half,
    /// which can be used to read and write the stream concurrently.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use std::io;
    ///
    /// use ylong_runtime::process::pty_process::Pty;
    ///
    /// async fn io_func() -> io::Result<()> {
    ///     let mut pty = Pty::new().expect("create Pty fail!");
    ///     let (read_pty, write_pty) = pty.into_split();
    ///     Ok(())
    /// }
    /// ```
    pub fn into_split(self) -> (SplitReadPty, SplitWritePty) {
        let arc = Arc::new(self);
        let read = SplitReadPty(Arc::clone(&arc));
        let write = SplitWritePty(Arc::clone(&arc));
        (read, write)
    }

    /// Unsplit `SplitReadPty` and `SplitWritePty` into a `Pty`
    ///
    /// # Panics
    /// If there are more than one copy of SplitReadPty or SplitWritePty, this
    /// method will panic
    ///
    /// # Example
    ///
    /// ```no_run
    /// use ylong_runtime::process::pty_process::Pty;
    /// let pty = Pty::new().unwrap();
    /// let pts = pty.pts().unwrap();
    /// let (read_pty, write_pty) = pty.into_split();
    /// let mut pty = Pty::unsplit(read_pty, write_pty).expect("unsplit fail!");
    /// ```
    pub fn unsplit(read_pty: SplitReadPty, write_pty: SplitWritePty) -> io::Result<Self> {
        let SplitReadPty(read_pty) = read_pty;
        let SplitWritePty(write_pty) = write_pty;
        if Arc::ptr_eq(&read_pty, &write_pty) {
            // drop SplitWritePty to ensure Arc::try_unwrap() successful.
            drop(write_pty);
            Ok(Arc::try_unwrap(read_pty)
                .expect("there are more than one copy of SplitRead or SplitWrite"))
        } else {
            Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "the SplitReadPty and the SplitWritePty come from different Pty",
            ))
        }
    }
}

impl From<Pty> for OwnedFd {
    fn from(value: Pty) -> Self {
        // io must be some until deregister
        value.0.io_take().expect("io deregister failed").into()
    }
}

impl AsFd for Pty {
    fn as_fd(&self) -> BorrowedFd<'_> {
        (*self.0).as_fd()
    }
}

impl AsRawFd for Pty {
    fn as_raw_fd(&self) -> RawFd {
        AsRawFd::as_raw_fd(&(*self.0))
    }
}

impl AsyncRead for Pty {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        self.0.poll_read(cx, buf)
    }
}

impl AsyncWrite for Pty {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        self.0.poll_write(cx, buf)
    }

    fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}

/// The child end of `Pty`
#[derive(Debug)]
pub struct Pts(super::sys::PtsInner);

impl Pts {
    fn new(pts_inner: super::sys::PtsInner) -> Self {
        Pts(pts_inner)
    }

    pub(crate) fn clone_stdio(&self) -> io::Result<Stdio> {
        self.0.clone_stdio()
    }

    pub(crate) fn session_leader(&self) -> impl FnMut() -> io::Result<()> {
        self.0.session_leader()
    }
}

/// Borrowed read half of a `Pty`
#[derive(Debug)]
pub struct BorrowReadPty<'a>(&'a Pty);

impl AsyncRead for BorrowReadPty<'_> {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        self.0 .0.poll_read(cx, buf)
    }
}

/// Borrowed write half of a `Pty`
#[derive(Debug)]
pub struct BorrowWritePty<'a>(&'a Pty);

impl BorrowWritePty<'_> {
    /// Changes the size of the terminal with `Pty`
    ///
    /// # Example
    ///
    /// ```no_run
    /// use ylong_runtime::process::pty_process::Pty;
    ///
    /// let mut pty = Pty::new().expect("create Pty fail!");
    /// let (read_pty, write_pty) = pty.split();
    /// write_pty.resize(24, 80, 0, 0).expect("resize set fail!");
    /// ```
    pub fn resize(
        &self,
        ws_row: u16,
        ws_col: u16,
        ws_xpixel: u16,
        ws_ypixel: u16,
    ) -> io::Result<()> {
        self.0.resize(ws_row, ws_col, ws_xpixel, ws_ypixel)
    }
}

impl AsyncWrite for BorrowWritePty<'_> {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        self.0 .0.poll_write(cx, buf)
    }

    fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}

/// Read half of a `Pty`
#[derive(Debug)]
pub struct SplitReadPty(Arc<Pty>);

impl AsyncRead for SplitReadPty {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        self.0 .0.poll_read(cx, buf)
    }
}

/// Write half of a `Pty`
#[derive(Debug)]
pub struct SplitWritePty(Arc<Pty>);

impl SplitWritePty {
    /// Changes the size of the terminal with `Pty`
    ///
    /// # Example
    ///
    /// ```no_run
    /// use ylong_runtime::process::pty_process::Pty;
    ///
    /// let mut pty = Pty::new().expect("create Pty fail!");
    /// let (read_pty, write_pty) = pty.into_split();
    /// write_pty.resize(24, 80, 0, 0).expect("resize set fail!");
    /// ```
    pub fn resize(
        &self,
        ws_row: u16,
        ws_col: u16,
        ws_xpixel: u16,
        ws_ypixel: u16,
    ) -> io::Result<()> {
        self.0.resize(ws_row, ws_col, ws_xpixel, ws_ypixel)
    }
}

impl AsyncWrite for SplitWritePty {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        self.0 .0.poll_write(cx, buf)
    }

    fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}
