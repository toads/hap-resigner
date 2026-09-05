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

//! Asynchronous signal handling in windows system.

mod registry;
mod winapi;

use std::io;
use std::sync::Once;

use registry::Registry;

use crate::signal::Signal;

/// Signal kind to listen for.
#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq)]
pub struct SignalKind(u32);

impl SignalKind {
    /// "ctrl-break" signal type.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ylong_runtime::signal::{signal, SignalKind};
    /// async fn io_func() {
    ///     let handle = ylong_runtime::spawn(async move {
    ///         let mut signal = signal(SignalKind::ctrl_break()).unwrap();
    ///         signal.recv().await;
    ///     });
    ///     let _ = ylong_runtime::block_on(handle);
    /// }
    /// ```
    pub const fn ctrl_break() -> SignalKind {
        SignalKind(winapi::CTRL_BREAK_EVENT)
    }

    /// "ctrl-close" signal type.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ylong_runtime::signal::{signal, SignalKind};
    /// async fn io_func() {
    ///     let handle = ylong_runtime::spawn(async move {
    ///         let mut signal = signal(SignalKind::ctrl_close()).unwrap();
    ///         signal.recv().await;
    ///     });
    ///     let _ = ylong_runtime::block_on(handle);
    /// }
    /// ```
    pub const fn ctrl_close() -> SignalKind {
        SignalKind(winapi::CTRL_CLOSE_EVENT)
    }

    /// "ctrl-c" signal type.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ylong_runtime::signal::{signal, SignalKind};
    /// async fn io_func() {
    ///     let handle = ylong_runtime::spawn(async move {
    ///         let mut signal = signal(SignalKind::ctrl_c()).unwrap();
    ///         signal.recv().await;
    ///     });
    ///     let _ = ylong_runtime::block_on(handle);
    /// }
    /// ```
    pub const fn ctrl_c() -> SignalKind {
        SignalKind(winapi::CTRL_C_EVENT)
    }

    /// "ctrl-logoff" signal type.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ylong_runtime::signal::{signal, SignalKind};
    /// async fn io_func() {
    ///     let handle = ylong_runtime::spawn(async move {
    ///         let mut signal = signal(SignalKind::ctrl_logoff()).unwrap();
    ///         signal.recv().await;
    ///     });
    ///     let _ = ylong_runtime::block_on(handle);
    /// }
    /// ```
    pub const fn ctrl_logoff() -> SignalKind {
        SignalKind(winapi::CTRL_LOGOFF_EVENT)
    }

    /// "ctrl-shutdown" signal type.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ylong_runtime::signal::{signal, SignalKind};
    /// async fn io_func() {
    ///     let handle = ylong_runtime::spawn(async move {
    ///         let mut signal = signal(SignalKind::ctrl_shutdown()).unwrap();
    ///         signal.recv().await;
    ///     });
    ///     let _ = ylong_runtime::block_on(handle);
    /// }
    /// ```
    pub const fn ctrl_shutdown() -> SignalKind {
        SignalKind(winapi::CTRL_SHUTDOWN_EVENT)
    }
}

#[derive(Default)]
pub(crate) struct SignalStream;

unsafe extern "system" fn signal_action(signal_kind: u32) -> i32 {
    let global = Registry::get_instance();
    global.broadcast(signal_kind as usize)
}

fn init_signal() -> io::Result<()> {
    static SIGNAL_ONCE: Once = Once::new();
    let mut register_res = Ok(());
    SIGNAL_ONCE.call_once(|| {
        let res = unsafe { winapi::SetConsoleCtrlHandler(Some(signal_action), 1) };
        register_res = if res != 0 {
            Ok(())
        } else {
            Err(io::Error::last_os_error())
        };
    });
    register_res
}

/// Creates a listener for the specified signal type.
///
/// # Errors
///
/// * If signal processing function registration failed.
///
/// # Examples
///
/// ```no run
/// use ylong_runtime::signal::{signal, SignalKind};
/// async fn io_func() {
///     let handle = ylong_runtime::spawn(async move {
///         let mut signal = signal(SignalKind::child()).unwrap();
///         signal.recv().await;
///     });
///     let _ = ylong_runtime::block_on(handle);
/// }
/// ```
pub fn signal(kind: SignalKind) -> io::Result<Signal> {
    init_signal()?;
    let registry = Registry::get_instance();
    Ok(Signal {
        inner: registry.listen_to_event(kind.0 as usize),
    })
}
