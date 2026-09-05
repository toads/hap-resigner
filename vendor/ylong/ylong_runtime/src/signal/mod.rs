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

#[cfg(unix)]
pub mod unix;
#[cfg(unix)]
pub use unix::{signal, SignalKind};

#[cfg(target_os = "windows")]
pub mod windows;
use std::task::{Context, Poll};

#[cfg(target_os = "windows")]
pub use windows::{signal, SignalKind};

use crate::sync::watch::Receiver;

/// A listener for monitoring operating system signals.
///
/// # Unix
/// This listener will merge signals of the same kind and receive them in a
/// stream, so for multiple triggers of the same signal, the receiver may only
/// receive one notification, which includes all triggered signal kinds.
///
/// When registering a listener for a certain kind of signal for the first time,
/// it will replace the default platform processing behavior. If some process
/// termination signals are triggered, the process will not be terminated
/// immediately, but will merge the signals into the stream and trigger them
/// uniformly, which will be captured and processed by the corresponding
/// receiver. Deconstructing the receiver does not reset the default platform
/// processing behavior.
///
/// # Examples
/// On Windows system
///
/// ```no run
/// use ylong_runtime::signal::{signal, SignalKind};
/// async fn io_func() {
///     let handle = ylong_runtime::spawn(async move {
///         let mut signal = signal(SignalKind::ctrl_c()).unwrap();
///         signal.recv().await;
///     });
///     let _ = ylong_runtime::block_on(handle);
/// }
/// ```
///
/// On Unix system
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
pub struct Signal {
    inner: Receiver<()>,
}

impl Signal {
    /// Waits for signal notification.
    ///
    /// # Examples
    /// On Windows system
    ///
    /// ```no run
    /// use ylong_runtime::signal::{signal, SignalKind};
    /// async fn io_func() {
    ///     let handle = ylong_runtime::spawn(async move {
    ///         let mut signal = signal(SignalKind::ctrl_c()).unwrap();
    ///         signal.recv().await;
    ///     });
    ///     let _ = ylong_runtime::block_on(handle);
    /// }
    /// ```
    ///
    /// On Unix system
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
    pub async fn recv(&mut self) {
        // The sender is saved in the registry of the global singleton and should not be
        // deconstructed.
        self.inner
            .notified()
            .await
            .unwrap_or_else(|e| panic!("Signal sender has been dropped, error: {e}"));
    }

    /// Polls to waits for signal notification.
    ///
    /// # Return value
    /// * `Poll::Pending` if no notification comes.
    /// * `Poll::Ready(())` if receiving a new signal notification.
    ///
    /// # Examples
    /// On Windows system
    ///
    /// ```no run
    /// use ylong_runtime::futures::poll_fn;
    /// use ylong_runtime::signal::{signal, SignalKind};
    /// async fn io_func() {
    ///     let handle = ylong_runtime::spawn(async move {
    ///         let mut signal = signal(SignalKind::ctrl_c()).unwrap();
    ///         poll_fn(|cx| signal.poll_recv(cx)).await;
    ///     });
    ///     let _ = ylong_runtime::block_on(handle);
    /// }
    /// ```
    ///
    /// On Unix system
    ///
    /// ```no run
    /// use ylong_runtime::futures::poll_fn;
    /// use ylong_runtime::signal::{signal, SignalKind};
    /// async fn io_func() {
    ///     let handle = ylong_runtime::spawn(async move {
    ///         let mut signal = signal(SignalKind::child()).unwrap();
    ///         poll_fn(|cx| signal.poll_recv(cx)).await;
    ///     });
    ///     let _ = ylong_runtime::block_on(handle);
    /// }
    /// ```
    pub fn poll_recv(&mut self, cx: &mut Context<'_>) -> Poll<()> {
        // The sender is saved in the registry of the global singleton and should not be
        // deconstructed.
        match self.inner.poll_notified(cx) {
            Poll::Ready(Ok(())) => Poll::Ready(()),
            Poll::Ready(Err(_)) => panic!("Signal sender has been dropped"),
            Poll::Pending => Poll::Pending,
        }
    }
}
