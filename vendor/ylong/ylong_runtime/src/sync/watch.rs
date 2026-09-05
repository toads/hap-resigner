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

//! Watch channel

use std::fmt::{Debug, Formatter};
use std::ops::Deref;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering::{Acquire, Release};
use std::sync::{Arc, RwLock, RwLockReadGuard};
use std::task::Poll::{Pending, Ready};
use std::task::{Context, Poll};

use crate::futures::poll_fn;
use crate::sync::error::{RecvError, SendError};
use crate::sync::wake_list::{ListItem, WakerList};

/// The least significant bit that marks the version of channel.
const VERSION_SHIFT: usize = 1;
/// The flag marks that channel is closed.
const CLOSED: usize = 1;

/// Creates a new watch channel with a `Sender` and `Receiver` handle pair.
///
/// The value sent by the `Sender` can be seen by all receivers, but only the
/// last value sent by `Sender` is visible to the `Receiver`.
///
/// # Examples
///
/// ```
/// use ylong_runtime::sync::watch;
/// async fn io_func() {
///     let (tx, mut rx) = watch::channel(1);
///     ylong_runtime::spawn(async move {
///         let _ = rx.notified().await;
///         assert_eq!(*rx.borrow(), 2);
///     });
///
///     let _ = tx.send(2);
/// }
/// ```
pub fn channel<T>(value: T) -> (Sender<T>, Receiver<T>) {
    let channel = Arc::new(Channel::new(value));
    let tx = Sender {
        channel: channel.clone(),
    };
    let rx = Receiver {
        channel,
        version: 0,
    };
    (tx, rx)
}

/// The sender of watch channel.
/// A [`Sender`] and [`Receiver`] handle pair is created by the [`channel`]
/// function.
///
/// # Examples
///
/// ```
/// use ylong_runtime::sync::watch;
/// async fn io_func() {
///     let (tx, mut rx) = watch::channel(1);
///     assert_eq!(tx.receiver_count(), 1);
///     ylong_runtime::spawn(async move {
///         let _ = rx.notified().await;
///         assert_eq!(*rx.borrow(), 2);
///     });
///
///     let _ = tx.send(2);
/// }
/// ```
#[derive(Debug)]
pub struct Sender<T> {
    channel: Arc<Channel<T>>,
}

impl<T> Sender<T> {
    /// Sends values to the associated [`Receiver`].
    ///
    /// An error containing the sent value would be returned if all receivers
    /// are dropped.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_runtime::sync::watch;
    /// async fn io_func() {
    ///     let (tx, mut rx) = watch::channel(1);
    ///     ylong_runtime::spawn(async move {
    ///         let _ = rx.notified().await;
    ///         assert_eq!(*rx.borrow(), 2);
    ///     });
    ///
    ///     let _ = tx.send(2);
    /// }
    /// ```
    pub fn send(&self, value: T) -> Result<(), SendError<T>> {
        if self.channel.rx_cnt.load(Acquire) == 0 {
            return Err(SendError(value));
        }
        let mut lock = self.channel.value.write().unwrap();
        *lock = value;
        self.channel.state.version_update();
        drop(lock);
        self.channel.waker_list.notify_all();
        Ok(())
    }

    /// Creates a new [`Receiver`] associated with oneself.
    ///
    /// The newly created receiver will mark all the values sent before as seen.
    ///
    /// This method can create a new receiver when there is no receiver
    /// available.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_runtime::sync::watch;
    /// async fn io_func() {
    ///     let (tx, mut rx) = watch::channel(1);
    ///     let mut rx2 = tx.subscribe();
    ///     assert_eq!(*rx.borrow(), 1);
    ///     assert_eq!(*rx2.borrow(), 1);
    ///     let _ = tx.send(2);
    ///     assert_eq!(*rx.borrow(), 2);
    ///     assert_eq!(*rx2.borrow(), 2);
    /// }
    /// ```
    pub fn subscribe(&self) -> Receiver<T> {
        let (value_version, _) = self.channel.state.load();
        self.channel.rx_cnt.fetch_add(1, Release);
        Receiver {
            channel: self.channel.clone(),
            version: value_version,
        }
    }

    /// Gets the number of receivers associated with oneself.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_runtime::sync::watch;
    /// async fn io_func() {
    ///     let (tx, rx) = watch::channel(1);
    ///     assert_eq!(tx.receiver_count(), 1);
    ///     let rx2 = tx.subscribe();
    ///     assert_eq!(tx.receiver_count(), 2);
    ///     let rx3 = rx.clone();
    ///     assert_eq!(tx.receiver_count(), 3);
    /// }
    /// ```
    pub fn receiver_count(&self) -> usize {
        self.channel.rx_cnt.load(Acquire)
    }
}

impl<T> Drop for Sender<T> {
    fn drop(&mut self) {
        self.channel.close();
    }
}

/// Reference to the inner value.
///
/// This reference will hold a read lock on the internal value, so holding this
/// reference will block the sender from sending data. When the watch channel
/// runs in an environment that allows !Send futures, you need to ensure that
/// the reference is not held across an. wait point to avoid deadlocks.
///
/// The priority policy of RwLock is consistent with the `std::RwLock`.
///
/// # Examples
///
/// ```
/// use ylong_runtime::sync::watch;
/// async fn io_func() {
///     let (tx, mut rx) = watch::channel(1);
///     let v1 = rx.borrow();
///     assert_eq!(*v1, 1);
///     assert!(!v1.is_notified());
///     drop(v1);
///
///     let _ = tx.send(2);
///     let v2 = rx.borrow_notify();
///     assert_eq!(*v2, 2);
///     assert!(v2.is_notified());
///     drop(v2);
///
///     let v3 = rx.borrow_notify();
///     assert_eq!(*v3, 2);
///     assert!(!v3.is_notified());
/// }
/// ```
#[derive(Debug)]
pub struct ValueRef<'a, T> {
    value: RwLockReadGuard<'a, T>,
    is_notified: bool,
}

impl<'a, T> ValueRef<'a, T> {
    fn new(value: RwLockReadGuard<'a, T>, is_notified: bool) -> ValueRef<'a, T> {
        ValueRef { value, is_notified }
    }

    /// Check if the borrowed value has been marked as seen.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_runtime::sync::watch;
    /// async fn io_func() {
    ///     let (tx, mut rx) = watch::channel(1);
    ///     let v1 = rx.borrow();
    ///     assert_eq!(*v1, 1);
    ///     assert!(!v1.is_notified());
    ///     drop(v1);
    ///
    ///     let _ = tx.send(2);
    ///     let v2 = rx.borrow_notify();
    ///     assert_eq!(*v2, 2);
    ///     assert!(v2.is_notified());
    ///     drop(v2);
    ///
    ///     let v3 = rx.borrow_notify();
    ///     assert_eq!(*v3, 2);
    ///     assert!(!v3.is_notified());
    /// }
    /// ```
    pub fn is_notified(&self) -> bool {
        self.is_notified
    }
}

impl<T> Deref for ValueRef<'_, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        self.value.deref()
    }
}

/// The receiver of watch channel.
/// A [`Sender`] and [`Receiver`] handle pair is created by the [`channel`]
/// function.
///
/// # Examples
///
/// ```
/// use ylong_runtime::sync::watch;
/// async fn io_func() {
///     let (tx, mut rx) = watch::channel(1);
///     ylong_runtime::spawn(async move {
///         let _ = rx.notified().await;
///         assert_eq!(*rx.borrow(), 2);
///     });
///
///     let _ = tx.send(2);
/// }
/// ```
#[derive(Debug)]
pub struct Receiver<T> {
    channel: Arc<Channel<T>>,
    version: usize,
}

impl<T> Receiver<T> {
    /// Check if [`Receiver`] has been notified of a new value that has not been
    /// marked as seen.
    ///
    /// An error would be returned if the channel is closed.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_runtime::sync::watch;
    /// async fn io_func() {
    ///     let (tx, mut rx) = watch::channel(1);
    ///     assert_eq!(rx.is_notified(), Ok(false));
    ///
    ///     let _ = tx.send(2);
    ///     assert_eq!(*rx.borrow(), 2);
    ///     assert_eq!(rx.is_notified(), Ok(true));
    ///
    ///     assert_eq!(*rx.borrow_notify(), 2);
    ///     assert_eq!(rx.is_notified(), Ok(false));
    ///
    ///     drop(tx);
    ///     assert!(rx.is_notified().is_err());
    /// }
    /// ```
    pub fn is_notified(&self) -> Result<bool, RecvError> {
        let (value_version, is_closed) = self.channel.state.load();
        if is_closed {
            return Err(RecvError);
        }
        Ok(self.version != value_version)
    }

    pub(crate) fn try_notified(&mut self) -> Option<Result<(), RecvError>> {
        let (value_version, is_closed) = self.channel.state.load();
        if self.version != value_version {
            self.version = value_version;
            return Some(Ok(()));
        }

        if is_closed {
            return Some(Err(RecvError));
        }

        None
    }

    /// Polls to receive a notification from the associated [`Sender`].
    ///
    /// When the sender has not yet sent a new message and the message in
    /// channel has seen, calling this method will return pending, and the
    /// waker from the Context will receive a wakeup when the message
    /// arrives or when the channel is closed. Multiple calls to this
    /// method, only the waker from the last call will receive a wakeup.
    ///
    /// # Return value
    /// * `Poll::Pending` if no new messages comes, but the channel is not
    ///   closed.
    /// * `Poll::Ready(Ok(T))` if receiving a new value or the value in channel
    ///   has not yet seen.
    /// * `Poll::Ready(Err(RecvError))` The sender has been dropped or the
    ///   channel is closed.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_runtime::futures::poll_fn;
    /// use ylong_runtime::sync::watch;
    /// async fn io_func() {
    ///     let (tx, mut rx) = watch::channel(1);
    ///     let handle = ylong_runtime::spawn(async move {
    ///         let _ = poll_fn(|cx| rx.poll_notified(cx)).await;
    ///         assert_eq!(*rx.borrow(), 2);
    ///     });
    ///     assert!(tx.send(2).is_ok());
    /// }
    /// ```
    pub fn poll_notified(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), RecvError>> {
        match self.try_notified() {
            Some(Ok(())) => return Ready(Ok(())),
            Some(Err(e)) => return Ready(Err(e)),
            None => {}
        }
        let wake = cx.waker().clone();
        self.channel.waker_list.insert(ListItem {
            wake,
            wait_permit: Arc::new(AtomicUsize::new(1)),
        });

        match self.try_notified() {
            Some(Ok(())) => Ready(Ok(())),
            Some(Err(e)) => Ready(Err(e)),
            None => Pending,
        }
    }

    /// Waits for a value change notification from the associated [`Sender`],
    /// and marks the value as seen then.
    ///
    /// If the channel has a value that has not yet seen, this method will
    /// return immediately and mark the value as seen. If the value in the
    /// channel has already been marked as seen, this method will wait
    /// asynchronously until the next new value arrives or the channel is
    /// closed.
    ///
    /// # Return value
    /// * `Ok(())` if receiving a new value or the value in channel has not yet
    ///   seen.
    /// * `Err(RecvError)` The sender has been dropped or the channel is closed.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_runtime::sync::watch;
    /// async fn io_func() {
    ///     let (tx, mut rx) = watch::channel(1);
    ///     ylong_runtime::spawn(async move {
    ///         let _ = rx.notified().await;
    ///         assert_eq!(*rx.borrow(), 2);
    ///     });
    ///
    ///     let _ = tx.send(2);
    /// }
    /// ```
    pub async fn notified(&mut self) -> Result<(), RecvError> {
        poll_fn(|cx| self.poll_notified(cx)).await
    }

    /// Gets a reference to the inner value.
    ///
    /// This method doesn't mark the value as seen, which means call to
    /// [`notified`] may return `Ok(())` immediately and call to [`is_notified`]
    /// may return `Ok(true)` after calling this method.
    ///
    /// The reference returned from this method will hold a read lock on the
    /// internal value, so holding this reference will block the sender from
    /// sending data. When the watch channel runs in an environment that
    /// allows !Send futures, you need to ensure that the reference is not held
    /// across an. wait point to avoid deadlocks.
    ///
    /// The priority policy of RwLock is consistent with the `std::RwLock`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_runtime::sync::watch;
    /// async fn io_func() {
    ///     let (tx, mut rx) = watch::channel(1);
    ///     ylong_runtime::spawn(async move {
    ///         let _ = rx.notified().await;
    ///         assert_eq!(*rx.borrow(), 2);
    ///     });
    ///
    ///     let _ = tx.send(2);
    /// }
    /// ```
    ///
    /// [`notified`]: Receiver::notified
    /// [`is_notified`]: Receiver::is_notified
    pub fn borrow(&self) -> ValueRef<'_, T> {
        let (value_version, _) = self.channel.state.load();
        let value = self.channel.value.read().unwrap();
        let is_notified = self.version != value_version;
        ValueRef::new(value, is_notified)
    }

    /// Gets a reference to the inner value and marks the value as seen.
    ///
    /// This method marks the value as seen, which means call to [`notified`]
    /// will wait until the next message comes and call to [`is_notified`] won't
    /// return `Ok(true)` after calling this method.
    ///
    /// The reference returned from this method will hold a read lock on the
    /// internal value, so holding this reference will block the sender from
    /// sending data. When the watch channel runs in an environment that
    /// allows !Send futures, you need to ensure that the reference is not held
    /// across an. wait point to avoid deadlocks.
    ///
    /// The priority policy of RwLock is consistent with the `std::RwLock`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_runtime::sync::watch;
    /// async fn io_func() {
    ///     let (tx, mut rx) = watch::channel(1);
    ///     ylong_runtime::spawn(async move {
    ///         let _ = rx.notified().await;
    ///         assert_eq!(*rx.borrow_notify(), 2);
    ///     });
    ///
    ///     let _ = tx.send(2);
    /// }
    /// ```
    ///
    /// [`notified`]: Receiver::notified
    /// [`is_notified`]: Receiver::is_notified
    pub fn borrow_notify(&mut self) -> ValueRef<'_, T> {
        let (value_version, _) = self.channel.state.load();
        let value = self.channel.value.read().unwrap();
        let is_notified = self.version != value_version;
        self.version = value_version;
        ValueRef::new(value, is_notified)
    }

    /// Checks whether the receiver and another receiver belong to the same
    /// channel.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_runtime::sync::watch;
    /// let (tx, rx) = watch::channel(1);
    /// let rx2 = rx.clone();
    /// assert!(rx.is_same(&rx2));
    /// ```
    pub fn is_same(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.channel, &other.channel)
    }
}

impl<T> Clone for Receiver<T> {
    fn clone(&self) -> Self {
        self.channel.rx_cnt.fetch_add(1, Release);
        Self {
            channel: self.channel.clone(),
            version: self.version,
        }
    }
}

impl<T> Drop for Receiver<T> {
    fn drop(&mut self) {
        self.channel.rx_cnt.fetch_sub(1, Release);
    }
}

struct State(AtomicUsize);

impl State {
    fn new() -> State {
        State(AtomicUsize::new(0))
    }

    fn version_update(&self) {
        self.0.fetch_add(1 << VERSION_SHIFT, Release);
    }

    fn load(&self) -> (usize, bool) {
        let state = self.0.load(Acquire);
        let version = state >> VERSION_SHIFT;
        let is_closed = state & CLOSED == CLOSED;
        (version, is_closed)
    }

    fn close(&self) {
        self.0.fetch_or(CLOSED, Release);
    }
}

struct Channel<T> {
    value: RwLock<T>,
    waker_list: WakerList,
    state: State,
    rx_cnt: AtomicUsize,
}

impl<T> Channel<T> {
    fn new(value: T) -> Channel<T> {
        Channel {
            value: RwLock::new(value),
            waker_list: WakerList::new(),
            state: State::new(),
            rx_cnt: AtomicUsize::new(1),
        }
    }

    fn close(&self) {
        self.state.close();
        self.waker_list.notify_all();
    }
}

impl<T: Debug> Debug for Channel<T> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let (value_version, is_closed) = self.state.load();
        f.debug_struct("Channel")
            .field("value", &self.value)
            .field("version", &value_version)
            .field("is_closed", &is_closed)
            .field("receiver_count", &self.rx_cnt.load(Acquire))
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::Ordering::Acquire;

    use crate::sync::error::RecvError;
    use crate::sync::watch;
    use crate::{block_on, spawn};

    /// UT test cases for `send()` and `try_notified()`.
    ///
    /// # Brief
    /// 1. Call channel to create a sender and a receiver handle pair.
    /// 2. Receiver tries receiving a change notification before the sender
    ///    sends one.
    /// 3. Receiver tries receiving a change notification after the sender sends
    ///    one.
    /// 4. Check if the test results are correct.
    /// 5. Receiver tries receiving a change notification after the sender
    ///    drops.
    #[test]
    fn send_try_notified() {
        let (tx, mut rx) = watch::channel("hello");
        assert_eq!(rx.try_notified(), None);
        assert!(tx.send("world").is_ok());
        assert_eq!(rx.try_notified(), Some(Ok(())));
        assert_eq!(*rx.borrow(), "world");

        drop(tx);
        assert_eq!(rx.try_notified(), Some(Err(RecvError)));
    }

    /// UT test cases for `send()` and async `notified()`.
    /// .
    /// # Brief
    /// 1. Call channel to create a sender and a receiver handle pair.
    /// 2. Sender sends message in one thread.
    /// 3. Receiver waits for a notification in another thread.
    /// 4. Check if the test results are correct.
    /// 5. Receiver waits for a notification in another thread after the sender
    ///    drops.
    #[test]
    fn send_notified_await() {
        let (tx, mut rx) = watch::channel("hello");
        assert!(tx.send("world").is_ok());
        drop(tx);
        let handle1 = spawn(async move {
            assert!(rx.notified().await.is_ok());
            assert_eq!(*rx.borrow(), "world");
            assert!(rx.notified().await.is_err());
        });
        let _ = block_on(handle1);
    }

    /// UT test cases for `send()` and `borrow_notify()`.
    ///
    /// # Brief
    /// 1. Call channel to create a sender and a receiver handle pair.
    /// 2. Check whether receiver contains a value which has not been seen
    ///    before and after `borrow()`.
    /// 3. Check whether receiver contains a value which has not been seen
    ///    before and after `borrow_notify()`.
    #[test]
    fn send_borrow_notify() {
        let (tx, mut rx) = watch::channel("hello");
        assert_eq!(rx.is_notified(), Ok(false));
        assert!(tx.send("world").is_ok());
        assert_eq!(rx.is_notified(), Ok(true));
        assert_eq!(*rx.borrow(), "world");
        assert_eq!(rx.is_notified(), Ok(true));
        assert_eq!(*rx.borrow_notify(), "world");
        assert_eq!(rx.is_notified(), Ok(false));
    }

    /// UT test cases for the count of the number of receivers.
    ///
    /// # Brief
    /// 1. Call channel to create a sender and a receiver handle pair.
    /// 2. Check whether receiver contains a value which has not been seen
    ///    before and after `borrow()`.
    /// 3. Check whether receiver contains a value which has not been seen
    ///    before and after `borrow_notify()`.
    #[test]
    fn receiver_count() {
        let (tx, rx) = watch::channel("hello");
        assert_eq!(tx.channel.rx_cnt.load(Acquire), 1);
        let rx2 = tx.subscribe();
        assert_eq!(tx.channel.rx_cnt.load(Acquire), 2);
        let rx3 = rx.clone();
        assert_eq!(tx.channel.rx_cnt.load(Acquire), 3);
        drop(rx);
        drop(rx2);
        drop(rx3);
        assert_eq!(tx.channel.rx_cnt.load(Acquire), 0);
    }
}
