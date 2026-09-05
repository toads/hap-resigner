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

use std::io::{ErrorKind, Read};

use ylong_io::{Interest, UnixStream};

#[cfg(not(feature = "ffrt"))]
use crate::executor::driver::Handle;
use crate::net::driver::SIGNAL_TOKEN;
use crate::signal::unix::registry::Registry;

pub(crate) struct SignalDriver {
    receiver: UnixStream,
}

cfg_ffrt! {
    use std::mem::MaybeUninit;
    static mut SIGNAL_DRIVER: MaybeUninit<SignalDriver> = MaybeUninit::uninit();
}

impl SignalDriver {
    pub(crate) fn broadcast(&mut self) {
        let mut buf = [0_u8; 8];
        loop {
            match self.receiver.read(&mut buf) {
                Ok(0) => panic!("EOF occurs in signal stream"),
                Ok(_) => {}
                Err(e) if e.kind() == ErrorKind::WouldBlock => break,
                Err(e) => panic!("Error occurs in signal stream: {e}"),
            }
        }
        Registry::get_instance().broadcast();
    }
}

#[cfg(not(feature = "ffrt"))]
impl SignalDriver {
    pub(crate) fn initialize(handle: &Handle) -> SignalDriver {
        // panic will occur when some errors like fds reaches the maximum limit
        // or insufficient memory occur. For more detailed errors, please refer to
        // `libc::fcntl`.
        let mut receiver = Registry::get_instance()
            .try_clone_stream()
            .unwrap_or_else(|e| panic!("Signal failed to clone UnixStream, {e}"));
        let _ = handle.io_register_with_token(
            &mut receiver,
            SIGNAL_TOKEN,
            Interest::READABLE | Interest::WRITABLE,
        );
        SignalDriver { receiver }
    }
}

#[cfg(feature = "ffrt")]
impl SignalDriver {
    pub(crate) fn get_mut_ref() -> &'static mut Self {
        SignalDriver::initialize();
        unsafe { &mut *SIGNAL_DRIVER.as_mut_ptr() }
    }

    pub(crate) fn initialize() {
        static ONCE: std::sync::Once = std::sync::Once::new();
        ONCE.call_once(|| unsafe {
            let mut receiver = Registry::get_instance()
                .try_clone_stream()
                .unwrap_or_else(|e| panic!("Signal failed to clone UnixStream, {e}"));
            let inner = crate::net::IoHandle::get_ref();
            inner.register_source_with_token(
                &mut receiver,
                SIGNAL_TOKEN,
                Interest::READABLE | Interest::WRITABLE,
            );
            SIGNAL_DRIVER = MaybeUninit::new(SignalDriver { receiver });
        });
    }
}
