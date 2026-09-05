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

use std::ffi::CStr;
use std::fs::OpenOptions;
use std::io;
use std::io::{Read, Write};
use std::os::fd::{AsFd, AsRawFd, BorrowedFd, FromRawFd, OwnedFd, RawFd};
use std::os::unix::ffi::OsStrExt;
use std::process::Stdio;

use ylong_io::sys::SourceFd;
use ylong_io::{Interest, Selector, Source, Token};

#[derive(Debug)]
pub(crate) struct PtyInner(OwnedFd);

impl PtyInner {
    pub(crate) fn open() -> io::Result<Self> {
        // Can not set CLOEXEC directly because it is linux-specific.
        let raw = syscall!(posix_openpt(libc::O_RDWR | libc::O_NOCTTY))?;

        syscall!(grantpt(raw))?;
        syscall!(unlockpt(raw))?;

        // Set CLOEXEC.
        let mut flags = syscall!(fcntl(raw, libc::F_GETFD))?;
        flags |= libc::O_CLOEXEC;
        syscall!(fcntl(raw, libc::F_SETFD, flags))?;

        let fd = unsafe { OwnedFd::from_raw_fd(raw) };
        Ok(Self(fd))
    }

    pub(crate) fn set_size(
        &self,
        ws_row: u16,
        ws_col: u16,
        ws_xpixel: u16,
        ws_ypixel: u16,
    ) -> io::Result<()> {
        let size = libc::winsize {
            ws_row,
            ws_col,
            ws_xpixel,
            ws_ypixel,
        };
        let raw = self.0.as_raw_fd();
        syscall!(ioctl(raw, libc::TIOCSWINSZ, std::ptr::addr_of!(size))).map(|_| ())
    }

    pub(crate) fn pts(&self, size: usize) -> io::Result<PtsInner> {
        let mut name_buf: Vec<libc::c_char> = vec![0; size];

        loop {
            let res = unsafe {
                libc::ptsname_r(
                    self.0.as_raw_fd(),
                    name_buf.as_mut_ptr().cast(),
                    name_buf.len(),
                )
            };
            match res {
                0 => {
                    name_buf.resize(name_buf.capacity(), 0);
                    break;
                }
                // If the vec's capacity is too small, double it.
                libc::ERANGE => {
                    name_buf.reserve(1);
                    name_buf.resize(name_buf.capacity(), 0)
                }
                _ => return Err(std::io::Error::last_os_error()),
            }
        }

        let name = unsafe { CStr::from_ptr(name_buf.as_ptr()) }.to_owned();
        let path = std::ffi::OsStr::from_bytes(name.as_bytes());

        let file = OpenOptions::new().read(true).write(true).open(path)?;
        Ok(PtsInner(file.into()))
    }

    pub(crate) fn set_nonblocking(&self) -> io::Result<()> {
        let mut flags = syscall!(fcntl(self.0.as_raw_fd(), libc::F_GETFL))?;
        flags |= libc::O_NONBLOCK;
        syscall!(fcntl(self.0.as_raw_fd(), libc::F_SETFL, flags)).map(|_| ())
    }
}

impl AsFd for PtyInner {
    fn as_fd(&self) -> BorrowedFd<'_> {
        self.0.as_fd()
    }
}

impl AsRawFd for PtyInner {
    fn as_raw_fd(&self) -> RawFd {
        self.0.as_raw_fd()
    }
}

impl From<PtyInner> for OwnedFd {
    fn from(value: PtyInner) -> Self {
        value.0
    }
}

macro_rules! impl_read_write {
    ($type:ty) => {
        impl Read for $type {
            fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
                syscall!(read(
                    self.0.as_raw_fd(),
                    buf.as_mut_ptr().cast::<libc::c_void>(),
                    buf.len() as libc::size_t
                ))
                .map(|res| res as usize)
            }
        }

        impl Write for $type {
            fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
                syscall!(write(
                    self.0.as_raw_fd(),
                    buf.as_ptr().cast::<libc::c_void>(),
                    buf.len() as libc::size_t
                ))
                .map(|res| res as usize)
            }

            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }
    };
}

impl_read_write!(PtyInner);
impl_read_write!(&PtyInner);

impl Source for PtyInner {
    fn register(
        &mut self,
        selector: &Selector,
        token: Token,
        interests: Interest,
    ) -> io::Result<()> {
        SourceFd(&self.get_fd()).register(selector, token, interests)
    }

    fn deregister(&mut self, selector: &Selector) -> io::Result<()> {
        SourceFd(&self.get_fd()).deregister(selector)
    }

    fn get_fd(&self) -> ylong_io::Fd {
        self.0.as_raw_fd()
    }
}

#[derive(Debug)]
pub(crate) struct PtsInner(OwnedFd);

impl PtsInner {
    pub(crate) fn clone_stdio(&self) -> io::Result<Stdio> {
        Ok(self.0.try_clone()?.into())
    }

    pub(crate) fn session_leader(&self) -> impl FnMut() -> io::Result<()> {
        let fd = self.0.as_raw_fd();
        move || {
            syscall!(setsid())?;
            syscall!(ioctl(fd, libc::TIOCSCTTY, std::ptr::null::<libc::c_int>()))?;
            Ok(())
        }
    }
}

impl From<PtsInner> for OwnedFd {
    fn from(value: PtsInner) -> Self {
        value.0
    }
}

impl AsFd for PtsInner {
    fn as_fd(&self) -> BorrowedFd<'_> {
        self.0.as_fd()
    }
}

impl AsRawFd for PtsInner {
    fn as_raw_fd(&self) -> RawFd {
        self.0.as_raw_fd()
    }
}

#[cfg(test)]
mod tests {
    use std::io::{Read, Write};
    use std::os::fd::{AsFd, AsRawFd, OwnedFd};

    use crate::process::pty_process::sys::PtyInner;

    /// Basic UT test cases for `PtyInner.pts()`.
    ///
    /// # Brief
    /// 1. Open a new `PtyInner`.
    /// 2. Call pts() with small size.
    /// 3. Check result is correct.
    #[test]
    fn ut_pty_pts_size_test() {
        let pty = PtyInner::open().unwrap();
        let pts = pty.pts(1);
        assert!(pts.is_ok());
        let pts = pts.unwrap();
        assert!(pts.as_fd().as_raw_fd() >= 0);
        assert!(pts.as_raw_fd() >= 0);
        let fd = OwnedFd::from(pts);
        assert!(fd.as_raw_fd() >= 0);
    }

    /// Basic UT test cases for `PtyInner` read and write.
    ///
    /// # Brief
    /// 1. Open a new `PtyInner`.
    /// 2. Write something into `PtyInner`.
    /// 3. Check read result is correct.
    #[test]
    fn ut_pty_read_write_test() {
        let mut pty = PtyInner::open().unwrap();
        let arg = "hello world!";
        pty.write_all(arg.as_bytes()).unwrap();

        let mut buf = [0; 12];
        pty.read_exact(&mut buf).unwrap();
        assert_eq!(buf, arg.as_bytes());
    }
}
