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

//! Asynchronous signal handling.

mod driver;
mod registry;

use std::io;
use std::io::{Error, ErrorKind};
use std::os::raw::c_int;

pub(crate) use driver::SignalDriver;
use registry::Registry;
use ylong_signal::SIGNAL_BLOCK_LIST;

use crate::signal::Signal;
use crate::sync::watch::Receiver;

/// Signal kind to listen for.
#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq)]
pub struct SignalKind(c_int);

impl SignalKind {
    pub(crate) fn get_max() -> i32 {
        #[cfg(target_os = "linux")]
        let max = libc::SIGRTMAX();
        #[cfg(not(target_os = "linux"))]
        let max = 33;
        max
    }

    /// Generates [`SignalKind`] from valid numeric value.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ylong_runtime::signal::{signal, SignalKind};
    /// async fn io_func() {
    ///     let signal_kind = SignalKind::from_raw(2);
    ///     let handle = ylong_runtime::spawn(async move {
    ///         let mut signal = signal(signal_kind).unwrap();
    ///         signal.recv().await;
    ///     });
    ///     let _ = ylong_runtime::block_on(handle);
    /// }
    /// ```
    pub const fn from_raw(signal_num: c_int) -> SignalKind {
        SignalKind(signal_num)
    }

    /// Gets the numeric value.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ylong_runtime::signal::SignalKind;
    /// async fn io_func() {
    ///     let signal_kind = SignalKind::interrupt();
    ///     assert_eq!(signal_kind.as_raw(), 2);
    /// }
    /// ```
    pub const fn as_raw(&self) -> c_int {
        self.0
    }

    /// SIGALRM signal.
    ///
    /// # Unix system
    /// Raised when a timer expires, typically used for timing operations. By
    /// default, it terminates the process.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ylong_runtime::signal::{signal, SignalKind};
    /// async fn io_func() {
    ///     let handle = ylong_runtime::spawn(async move {
    ///         let mut signal = signal(SignalKind::alarm()).unwrap();
    ///         signal.recv().await;
    ///     });
    ///     let _ = ylong_runtime::block_on(handle);
    /// }
    /// ```
    pub const fn alarm() -> SignalKind {
        SignalKind(libc::SIGALRM as c_int)
    }

    /// SIGCHLD signal.
    ///
    /// # Unix system
    /// Received by the parent process when a child process terminates or stops.
    /// By default, it is ignored.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ylong_runtime::signal::{signal, SignalKind};
    /// async fn io_func() {
    ///     let handle = ylong_runtime::spawn(async move {
    ///         let mut signal = signal(SignalKind::child()).unwrap();
    ///         signal.recv().await;
    ///     });
    ///     let _ = ylong_runtime::block_on(handle);
    /// }
    /// ```
    pub const fn child() -> SignalKind {
        SignalKind(libc::SIGCHLD as c_int)
    }

    /// SIGHUP signal.
    ///
    /// # Unix system
    /// Raised when the terminal connection is disconnected, usually sent to all
    /// members of the foreground process group. By default, it is ignored.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ylong_runtime::signal::{signal, SignalKind};
    /// async fn io_func() {
    ///     let handle = ylong_runtime::spawn(async move {
    ///         let mut signal = signal(SignalKind::hangup()).unwrap();
    ///         signal.recv().await;
    ///     });
    ///     let _ = ylong_runtime::block_on(handle);
    /// }
    /// ```
    pub const fn hangup() -> SignalKind {
        SignalKind(libc::SIGHUP as c_int)
    }

    /// SIGINT signal.
    ///
    /// # Unix system
    /// Raised when the user interrupts the process. By default, it terminates
    /// the process.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ylong_runtime::signal::{signal, SignalKind};
    /// async fn io_func() {
    ///     let handle = ylong_runtime::spawn(async move {
    ///         let mut signal = signal(SignalKind::interrupt()).unwrap();
    ///         signal.recv().await;
    ///     });
    ///     let _ = ylong_runtime::block_on(handle);
    /// }
    /// ```
    pub const fn interrupt() -> SignalKind {
        SignalKind(libc::SIGINT as c_int)
    }

    /// SIGIO signal.
    ///
    /// # Unix system
    /// Sent by the kernel to the process when I/O is available. By default,it
    /// is ignored.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ylong_runtime::signal::{signal, SignalKind};
    /// async fn io_func() {
    ///     let handle = ylong_runtime::spawn(async move {
    ///         let mut signal = signal(SignalKind::io()).unwrap();
    ///         signal.recv().await;
    ///     });
    ///     let _ = ylong_runtime::block_on(handle);
    /// }
    /// ```
    pub const fn io() -> SignalKind {
        SignalKind(libc::SIGIO as c_int)
    }

    /// SIGPIPE signal.
    ///
    /// # Unix system
    /// Sent by the kernel to the process when writing to a closed pipe or
    /// socket. By default, it terminates the process.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ylong_runtime::signal::{signal, SignalKind};
    /// async fn io_func() {
    ///     let handle = ylong_runtime::spawn(async move {
    ///         let mut signal = signal(SignalKind::pipe()).unwrap();
    ///         signal.recv().await;
    ///     });
    ///     let _ = ylong_runtime::block_on(handle);
    /// }
    /// ```
    pub const fn pipe() -> SignalKind {
        SignalKind(libc::SIGPIPE as c_int)
    }

    /// SIGQUIT signal.
    ///
    /// # Unix system
    /// Raised when the user requests process termination and generate a core
    /// dump. By default, it terminates the process.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ylong_runtime::signal::{signal, SignalKind};
    /// async fn io_func() {
    ///     let handle = ylong_runtime::spawn(async move {
    ///         let mut signal = signal(SignalKind::quit()).unwrap();
    ///         signal.recv().await;
    ///     });
    ///     let _ = ylong_runtime::block_on(handle);
    /// }
    /// ```
    pub const fn quit() -> SignalKind {
        SignalKind(libc::SIGQUIT as c_int)
    }

    /// SIGTERM signal.
    ///
    /// # Unix system
    /// A termination signal sent by the user to the process. By default, it
    /// terminates the process.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ylong_runtime::signal::{signal, SignalKind};
    /// async fn io_func() {
    ///     let handle = ylong_runtime::spawn(async move {
    ///         let mut signal = signal(SignalKind::terminate()).unwrap();
    ///         signal.recv().await;
    ///     });
    ///     let _ = ylong_runtime::block_on(handle);
    /// }
    /// ```
    pub const fn terminate() -> SignalKind {
        SignalKind(libc::SIGTERM as c_int)
    }

    /// SIGUSR1 signal.
    ///
    /// # Unix system
    /// User-defined signal that can be used for custom operations. By default,
    /// it terminates the process.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ylong_runtime::signal::{signal, SignalKind};
    /// async fn io_func() {
    ///     let handle = ylong_runtime::spawn(async move {
    ///         let mut signal = signal(SignalKind::user_defined1()).unwrap();
    ///         signal.recv().await;
    ///     });
    ///     let _ = ylong_runtime::block_on(handle);
    /// }
    /// ```
    pub const fn user_defined1() -> SignalKind {
        SignalKind(libc::SIGUSR1 as c_int)
    }

    /// SIGUSR2 signal.
    ///
    /// # Unix system
    /// User-defined signal that can be used for custom operations. By default,
    /// it terminates the process.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ylong_runtime::signal::{signal, SignalKind};
    /// async fn io_func() {
    ///     let handle = ylong_runtime::spawn(async move {
    ///         let mut signal = signal(SignalKind::user_defined2()).unwrap();
    ///         signal.recv().await;
    ///     });
    ///     let _ = ylong_runtime::block_on(handle);
    /// }
    /// ```
    pub const fn user_defined2() -> SignalKind {
        SignalKind(libc::SIGUSR2 as c_int)
    }

    /// SIGWINCH signal.
    ///
    /// # Unix system
    /// Sent by the kernel to all members of the foreground process group when
    /// the terminal window size changes. By default, it is ignored.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ylong_runtime::signal::{signal, SignalKind};
    /// async fn io_func() {
    ///     let handle = ylong_runtime::spawn(async move {
    ///         let mut signal = signal(SignalKind::window_change()).unwrap();
    ///         signal.recv().await;
    ///     });
    ///     let _ = ylong_runtime::block_on(handle);
    /// }
    /// ```
    pub const fn window_change() -> SignalKind {
        SignalKind(libc::SIGWINCH as c_int)
    }

    /// Checks whether the signal is forbidden.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ylong_runtime::signal::SignalKind;
    /// async fn io_func() {
    ///     // SIGSEGV
    ///     let signal_kind = SignalKind::from_raw(11);
    ///     assert!(signal_kind.is_forbidden());
    /// }
    /// ```
    pub fn is_forbidden(&self) -> bool {
        if self.0 < 0 || self.0 > SignalKind::get_max() {
            return true;
        }
        SIGNAL_BLOCK_LIST.contains(&self.0)
    }
}

impl From<c_int> for SignalKind {
    fn from(value: c_int) -> Self {
        Self::from_raw(value)
    }
}

impl From<SignalKind> for c_int {
    fn from(value: SignalKind) -> Self {
        value.as_raw()
    }
}

/// A callback processing function registered for specific a signal kind.
///
/// Operations in this method should be async-signal safe.
fn signal_action(signal_kind: c_int) {
    let global = Registry::get_instance();
    global.notify_event(signal_kind as usize);
    let _ = global.write(&[1]);
}

pub(crate) fn signal_return_watch(kind: SignalKind) -> io::Result<Receiver<()>> {
    if kind.is_forbidden() {
        return Err(Error::new(ErrorKind::Other, "Invalid signal kind"));
    }

    let registry = Registry::get_instance();
    let event = registry.get_event(kind.0 as usize);
    event.register(kind.0, move || signal_action(kind.0))?;
    Ok(registry.listen_to_event(kind.0 as usize))
}

/// Creates a listener for the specified signal type.
///
/// # Notice
/// This method will create a streaming listener bound to the runtime, which
/// will replace the default platform processing behavior and it will not be
/// reset after the receiver is destroyed. The same signal can be registered
/// multiple times, and when the signal is triggered, all receivers will receive
/// a notification.
///
/// # Errors
///
/// * If signal processing function registration failed.
/// * If the signal is one of [`SIGNAL_BLOCK_LIST`].
///
/// # Panics
///
/// This function panics if there is no ylong_runtime in the environment.
///
/// # Examples
///
/// ```no_run
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
    #[cfg(feature = "ffrt")]
    let _ = SignalDriver::get_mut_ref();
    let receiver = signal_return_watch(kind)?;
    Ok(Signal { inner: receiver })
}

#[cfg(test)]
mod tests {
    use std::os::raw::c_int;

    use ylong_signal::SIGNAL_BLOCK_LIST;

    use crate::futures::poll_fn;
    use crate::signal::unix::signal_return_watch;
    use crate::signal::{signal, SignalKind};

    /// UT test cases of `SignalKind` conversion.
    ///
    /// # Brief
    /// 1. Check the trait `From<c_int>` for `SignalKind`.
    /// 2. Check the trait `From<SignalKind>` for `c_int`.
    /// 3. Check the method `from_raw` of `SignalKind`.
    /// 4. Check the method `as_raw` of `SignalKind`.
    #[test]
    fn ut_signal_from_and_into_c_int() {
        assert_eq!(SignalKind::from(1), SignalKind::hangup());
        assert_eq!(c_int::from(SignalKind::hangup()), 1);
        assert_eq!(SignalKind::from_raw(2), SignalKind::interrupt());
        assert_eq!(SignalKind::interrupt().as_raw(), 2);
    }

    /// UT test cases for `signal_return_watch` with forbidden input.
    ///
    /// # Brief
    /// 1. Generate a forbidden kind of signal.
    /// 2. Call `signal_return_watch` and check the result.
    #[test]
    fn ut_signal_forbidden_input() {
        let signal_kind = SignalKind::from_raw(SIGNAL_BLOCK_LIST[0]);
        assert!(signal_return_watch(signal_kind).is_err());
    }

    /// UT test cases for `recv` and `poll_recv`.
    ///
    /// # Brief
    /// 1. Generate a kind of signal.
    /// 2. Send notification signals and try receiving them through `recv` and
    ///    `poll_recv`.
    #[test]
    fn ut_signal_recv_and_poll_recv() {
        let mut handles = Vec::new();
        handles.push(crate::spawn(async move {
            let mut signal = signal(SignalKind::alarm()).unwrap();
            unsafe { libc::raise(libc::SIGALRM) };
            signal.recv().await;
        }));
        handles.push(crate::spawn(async move {
            let mut signal = signal(SignalKind::alarm()).unwrap();
            unsafe { libc::raise(libc::SIGALRM) };
            poll_fn(|cx| signal.poll_recv(cx)).await;
        }));
    }

    /// UT test cases for SIGALRM signal.
    ///
    /// # Brief
    /// 1. Generate the SIGALRM signal.
    /// 2. Check the function of `signal` for the SIGALRM signal.
    #[test]
    fn ut_signal_alarm() {
        let handle = crate::spawn(async move {
            let mut signal = signal(SignalKind::alarm()).unwrap();
            unsafe { libc::raise(libc::SIGALRM) };
            signal.recv().await;
        });
        let _ = crate::block_on(handle);
    }

    /// UT test cases for SIGCHLD signal.
    ///
    /// # Brief
    /// 1. Generate the SIGCHLD signal.
    /// 2. Check the function of `signal` for the SIGCHLD signal.
    #[test]
    fn ut_signal_child() {
        let handle = crate::spawn(async move {
            let mut signal = signal(SignalKind::child()).unwrap();
            unsafe { libc::raise(libc::SIGCHLD) };
            signal.recv().await;
        });
        let _ = crate::block_on(handle);
    }

    /// UT test cases for SIGHUP signal.
    ///
    /// # Brief
    /// 1. Generate the SIGHUP signal.
    /// 2. Check the function of `signal` for the SIGHUP signal.
    #[test]
    fn ut_signal_hangup() {
        let handle = crate::spawn(async move {
            let mut signal = signal(SignalKind::hangup()).unwrap();
            unsafe { libc::raise(libc::SIGHUP) };
            signal.recv().await;
        });
        let _ = crate::block_on(handle);
    }

    /// UT test cases for SIGINT signal.
    ///
    /// # Brief
    /// 1. Generate the SIGINT signal.
    /// 2. Check the function of `signal` for the SIGINT signal.
    #[test]
    fn ut_signal_interrupt() {
        let handle = crate::spawn(async move {
            let mut signal = signal(SignalKind::interrupt()).unwrap();
            unsafe { libc::raise(libc::SIGINT) };
            signal.recv().await;
        });
        let _ = crate::block_on(handle);
    }

    /// UT test cases for SIGIO signal.
    ///
    /// # Brief
    /// 1. Generate the SIGIO signal.
    /// 2. Check the function of `signal` for the SIGIO signal.
    #[test]
    fn ut_signal_io() {
        let handle = crate::spawn(async move {
            let mut signal = signal(SignalKind::io()).unwrap();
            unsafe { libc::raise(libc::SIGIO) };
            signal.recv().await;
        });
        let _ = crate::block_on(handle);
    }

    /// UT test cases for SIGPIPE signal.
    ///
    /// # Brief
    /// 1. Generate the SIGPIPE signal.
    /// 2. Check the function of `signal` for the SIGPIPE signal.
    #[test]
    fn ut_signal_pipe() {
        let handle = crate::spawn(async move {
            let mut signal = signal(SignalKind::pipe()).unwrap();
            unsafe { libc::raise(libc::SIGPIPE) };
            signal.recv().await;
        });
        let _ = crate::block_on(handle);
    }

    /// UT test cases for SIGTERM signal.
    ///
    /// # Brief
    /// 1. Generate the SIGTERM signal.
    /// 2. Check the function of `signal` for the SIGTERM signal.
    #[test]
    fn ut_signal_terminate() {
        let handle = crate::spawn(async move {
            let mut signal = signal(SignalKind::terminate()).unwrap();
            unsafe { libc::raise(libc::SIGTERM) };
            signal.recv().await;
        });
        let _ = crate::block_on(handle);
    }

    /// UT test cases for SIGUSR1 signal.
    ///
    /// # Brief
    /// 1. Generate the SIGUSR1 signal.
    /// 2. Check the function of `signal` for the SIGUSR1 signal.
    #[test]
    fn ut_signal_user_defined1() {
        let handle = crate::spawn(async move {
            let mut signal = signal(SignalKind::user_defined1()).unwrap();
            unsafe { libc::raise(libc::SIGUSR1) };
            signal.recv().await;
        });
        let _ = crate::block_on(handle);
    }

    /// UT test cases for SIGUSR2 signal.
    ///
    /// # Brief
    /// 1. Generate the SIGUSR2 signal.
    /// 2. Check the function of `signal` for the SIGUSR2 signal.
    #[test]
    fn ut_signal_user_defined2() {
        let handle = crate::spawn(async move {
            let mut signal = signal(SignalKind::user_defined2()).unwrap();
            unsafe { libc::raise(libc::SIGUSR2) };
            signal.recv().await;
        });
        let _ = crate::block_on(handle);
    }

    /// UT test cases for SIGWINCH signal.
    ///
    /// # Brief
    /// 1. Generate the SIGWINCH signal.
    /// 2. Check the function of `signal` for the SIGWINCH signal.
    #[test]
    fn ut_signal_window_change() {
        let handle = crate::spawn(async move {
            let mut signal = signal(SignalKind::window_change()).unwrap();
            unsafe { libc::raise(libc::SIGWINCH) };
            signal.recv().await;
        });
        let _ = crate::block_on(handle);
    }
}
