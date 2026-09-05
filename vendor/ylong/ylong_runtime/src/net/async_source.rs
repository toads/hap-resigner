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

use std::fmt::{Debug, Formatter};
use std::io;
use std::io::{Read, Write};
use std::mem::MaybeUninit;
use std::ops::Deref;
use std::sync::Arc;
use std::task::{Context, Poll};

#[cfg(target_os = "linux")]
use libc::{gid_t, uid_t};
use ylong_io::{Interest, Source};

use crate::executor::Handle;
use crate::io::{poll_ready, ReadBuf};
use crate::net::{ReadyEvent, ScheduleIO};
use crate::util::slab::Ref;

/// Wrapper that turns a sync `Source` io into an async one. This struct
/// interacts with the reactor of the runtime.
pub(crate) struct AsyncSource<E: Source> {
    /// Sync io that implements `Source` trait.
    io: Option<E>,

    /// Entry list of the runtime's reactor, `AsyncSource` object will be
    /// registered into it when created.
    pub(crate) entry: Ref<ScheduleIO>,

    /// Handle to the IO Driver, used for deregistration
    pub(crate) handle: Arc<Handle>,
}

impl<E: Source> AsyncSource<E> {
    #[cfg(target_os = "linux")]
    pub fn fchown(&self, uid: uid_t, gid: gid_t) -> io::Result<()> {
        syscall!(fchown(self.get_fd(), uid, gid))?;
        Ok(())
    }

    /// Wraps a `Source` object into an `AsyncSource`. When the `AsyncSource`
    /// object is created, it's fd will be registered into runtime's
    /// reactor.
    ///
    /// If `interest` passed in is None, the interested event for fd
    /// registration will be both readable and writable.
    ///
    /// # Error
    ///
    /// If no reactor is found or fd registration fails, an error will be
    /// returned.
    pub fn new(mut io: E, interest: Option<Interest>) -> io::Result<AsyncSource<E>> {
        let inner = Handle::get_handle()?;

        let interest = interest.unwrap_or_else(|| Interest::READABLE | Interest::WRITABLE);
        let entry = inner.io_register(&mut io, interest)?;
        Ok(AsyncSource {
            io: Some(io),
            entry,
            handle: inner,
        })
    }

    /// Asynchronously waits for events to happen. If the io returns
    /// `EWOULDBLOCK`, the readiness of the io will be reset. Otherwise, the
    /// corresponding event will be returned.
    pub(crate) async fn async_process<F, R>(&self, interest: Interest, mut op: F) -> io::Result<R>
    where
        F: FnMut() -> io::Result<R>,
    {
        loop {
            let ready = self.entry.readiness(interest).await?;
            match op() {
                Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                    self.entry.clear_readiness(ready);
                }
                x => return x,
            }
        }
    }

    #[cfg(target_os = "linux")]
    cfg_process! {
        /// Deregisters the io and return it.
        pub(crate) fn io_take(mut self) -> io::Result<E> {
            // before AsyncSource drop, io is always Some().
            let mut io = self.io.take().unwrap();
            self.handle.io_deregister(&mut io)?;
            Ok(io)
        }
    }

    cfg_net! {
        pub(crate) fn poll_ready(
            &self,
            cx: &mut Context<'_>,
            interest: Interest,
        ) -> Poll<io::Result<ReadyEvent>> {
            let ready = self.entry.poll_readiness(cx, interest);
            let x = match ready {
                Poll::Ready(x) => x,
                Poll::Pending => return Poll::Pending,
            };

            Poll::Ready(Ok(x))
        }

        pub(crate) fn poll_io<R>(
            &self,
            cx: &mut Context<'_>,
            interest: Interest,
            mut f: impl FnMut() -> io::Result<R>,
        ) -> Poll<io::Result<R>> {
            loop {
                let ready = poll_ready!(self.poll_ready(cx, interest))?;

                match f() {
                    Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                        self.entry.clear_readiness(ready);
                    }
                    x => return Poll::Ready(x),
                }
            }
        }

        pub(crate) fn try_io<R> (
            &self,
            interest: Interest,
            mut f: impl FnMut() -> io::Result<R>,
        ) -> io::Result<R> {
            let event = self.entry.get_readiness(interest);

            if event.ready.is_empty() {
                return Err(io::ErrorKind::WouldBlock.into());
            }

            match f() {
                Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                    self.entry.clear_readiness(event);
                    Err(io::ErrorKind::WouldBlock.into())
                }
                res => res,
            }
        }

        #[inline]
        pub(crate) fn poll_read_io<R>(
            &self,
            cx: &mut Context<'_>,
            f: impl FnMut() -> io::Result<R>,
        ) -> Poll<io::Result<R>> {
            self.poll_io(cx, Interest::READABLE, f)
        }

        #[inline]
        pub(crate) fn poll_write_io<R>(
            &self,
            cx: &mut Context<'_>,
            f: impl FnMut() -> io::Result<R>,
        ) -> Poll<io::Result<R>> {
            self.poll_io(cx, Interest::WRITABLE, f)
        }

        pub(crate) fn poll_read<'a>(
            &'a self,
            cx: &mut Context<'_>,
            buf: &mut ReadBuf<'_>,
        ) -> Poll<io::Result<()>>
        where
            &'a E: io::Read + 'a,
        {
            let ret = self.poll_read_io(cx, || unsafe {
                let slice = &mut *(buf.unfilled_mut() as *mut [MaybeUninit<u8>] as *mut [u8]);
                // before AsyncSource drop, io is always Some().
                self.io.as_ref().unwrap().read(slice)
            });
            let r_len = match ret {
                Poll::Ready(Ok(x)) => x,
                Poll::Ready(Err(e)) => return Poll::Ready(Err(e)),
                Poll::Pending => return Poll::Pending,
            };
            buf.assume_init(r_len);
            buf.advance(r_len);

            Poll::Ready(Ok(()))
        }

        pub(crate) fn poll_write<'a>(
            &'a self,
            cx: &mut Context<'_>,
            buf: &[u8],
        ) -> Poll<io::Result<usize>>
        where
            &'a E: io::Write + 'a,
        {
            self.poll_write_io(cx, || {
                // before AsyncSource drop, io is always Some().
                self.io.as_ref().unwrap().write(buf)
            })
        }

        pub(crate) fn poll_write_vectored<'a>(
            &'a self,
            cx: &mut Context<'_>,
            bufs: &[io::IoSlice<'_>],
        ) -> Poll<io::Result<usize>>
        where
            &'a E: io::Write + 'a,
        {
            self.poll_write_io(cx, || {
                // before AsyncSource drop, io is always Some().
                self.io.as_ref().unwrap().write_vectored(bufs)
            })
        }
    }
}

impl<E: Source + Debug> Debug for AsyncSource<E> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AsyncSource").field("io", &self.io).finish()
    }
}

impl<E: Source> Deref for AsyncSource<E> {
    type Target = E;

    fn deref(&self) -> &Self::Target {
        // before AsyncSource drop, io is always Some().
        self.io.as_ref().unwrap()
    }
}

// Deregisters fd when the `AsyncSource` object get dropped.
impl<E: Source> Drop for AsyncSource<E> {
    fn drop(&mut self) {
        if let Some(mut io) = self.io.take() {
            let _ = self.handle.io_deregister(&mut io);
        }
    }
}
