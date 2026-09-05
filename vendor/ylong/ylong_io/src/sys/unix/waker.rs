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

use std::io;

use crate::{Selector, Token};

macro_rules! cfg_linux {
    ($($item:item)*) => {
        $(
            #[cfg(target_os = "linux")]
            $item
        )*
    }
}

macro_rules! cfg_macos {
    ($($item:item)*) => {
        $(
            #[cfg(target_os = "macos")]
            $item
        )*
    }
}

cfg_linux!(
    use std::fs::File;
    use std::io::{Read, Write};
    use std::os::unix::io::FromRawFd;
    use crate::Interest;

    /// In Linux, `eventfd` is used to implement asynchronous wake-up. It is a
    /// 64-bit counter. A fixed 8-byte (64-bit) unsigned integer is written to
    /// ensure wake-up reliability.
    #[derive(Debug)]
    pub(crate) struct WakerInner {
        fd: File,
    }

    impl WakerInner {
        pub(crate) fn new(selector: &Selector, token: Token) -> io::Result<WakerInner> {
            let fd = unsafe { libc::eventfd(0, libc::EFD_CLOEXEC | libc::EFD_NONBLOCK) };
            let file = unsafe { File::from_raw_fd(fd) };
            if fd == -1 {
                let err = io::Error::last_os_error();
                Err(err)
            } else {
                selector
                    .register(fd, token, Interest::READABLE)
                    .map(|()| WakerInner { fd: file })
            }
        }

        pub(crate) fn wake(&self) -> io::Result<()> {
            let buf: [u8; 8] = 1u64.to_ne_bytes();
            match (&self.fd).write(&buf) {
                Ok(_) => Ok(()),
                Err(ref err) if err.kind() == io::ErrorKind::WouldBlock => {
                    let mut buf: [u8; 8] = 0u64.to_ne_bytes();
                    match (&self.fd).read(&mut buf) {
                        Err(err) if err.kind() != io::ErrorKind::WouldBlock => Err(err),
                        _ => self.wake(),
                    }
                }
                Err(err) => Err(err),
            }
        }
    }
);

cfg_macos!(
    /// In MacOs, kqueue with `EVFILT_USER` is used to implement asynchronous wake-up.
    #[derive(Debug)]
    pub(crate) struct WakerInner {
        selector: Selector,
        token: Token,
    }

    impl WakerInner {
        pub(crate) fn new(selector: &Selector, token: Token) -> io::Result<WakerInner> {
            let selector = selector.try_clone()?;
            selector.register_waker(token)?;
            Ok(WakerInner { selector, token })
        }

        pub(crate) fn wake(&self) -> io::Result<()> {
            self.selector.wake(self.token)
        }
    }
);
