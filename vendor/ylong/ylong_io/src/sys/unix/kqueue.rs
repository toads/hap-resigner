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

use std::ffi::c_void;
use std::ops::{Deref, DerefMut};
use std::os::unix::io::RawFd;
use std::time::Duration;
use std::{cmp, io, mem, ptr};

use libc::{c_int, uintptr_t};

use crate::{EventTrait, Interest, Token};

/// An wrapper for different OS polling system.
/// Linux: epoll
/// Windows: iocp
/// macos: kqueue
#[derive(Debug)]
pub struct Selector {
    kq: RawFd,
}

impl Selector {
    /// Creates a new Selector.
    ///
    /// # Error
    /// If the underlying syscall fails, returns the corresponding error.
    pub fn new() -> io::Result<Selector> {
        let kq = syscall!(kqueue())?;
        // make sure the fd closed when child process executes
        syscall!(fcntl(kq, libc::F_SETFD, libc::FD_CLOEXEC))?;

        Ok(Selector { kq })
    }

    /// Waits for io events to come within a time limit.
    pub fn select(&self, events: &mut Events, timeout: Option<Duration>) -> io::Result<()> {
        events.clear();

        let timeout = timeout.map(|time| libc::timespec {
            tv_sec: cmp::min(time.as_secs(), libc::time_t::MAX as u64) as libc::time_t,
            // the cast is safe cause c_long::max > nanoseconds per second
            tv_nsec: libc::c_long::from(time.subsec_nanos() as i32),
        });

        let timeout_ptr = match timeout.as_ref() {
            Some(t) => t as *const libc::timespec,
            None => ptr::null_mut(),
        };

        let n_events = syscall!(kevent(
            self.kq,
            ptr::null(),
            0,
            events.as_mut_ptr(),
            events.capacity() as c_int,
            timeout_ptr,
        ))?;
        unsafe { events.set_len(n_events as usize) };

        Ok(())
    }

    /// Registers the fd with specific interested events
    pub fn register(&self, fd: RawFd, token: Token, interests: Interest) -> io::Result<()> {
        let flags = libc::EV_CLEAR | libc::EV_RECEIPT | libc::EV_ADD;
        let mut events = Vec::with_capacity(2);
        if interests.is_readable() {
            let kevent = kevent_new(fd, libc::EVFILT_READ, flags, token.0);
            events.push(kevent);
        }

        if interests.is_writable() {
            let kevent = kevent_new(fd, libc::EVFILT_WRITE, flags, token.0);
            events.push(kevent);
        }

        kevent_register(self.kq, events.as_mut_slice())?;
        kevent_check_error(events.as_mut_slice(), &[libc::EPIPE as i64])
    }

    /// Re-registers the fd with specific interested events
    pub fn reregister(&self, fd: i32, token: Token, interests: Interest) -> io::Result<()> {
        let flags = libc::EV_CLEAR | libc::EV_RECEIPT;
        let mut events = Vec::with_capacity(2);

        let r_flags = match interests.is_readable() {
            true => flags | libc::EV_ADD,
            false => flags | libc::EV_DELETE,
        };

        let w_flags = match interests.is_writable() {
            true => flags | libc::EV_ADD,
            false => flags | libc::EV_DELETE,
        };

        events.push(kevent_new(fd, libc::EVFILT_READ, r_flags, token.0));
        events.push(kevent_new(fd, libc::EVFILT_WRITE, w_flags, token.0));
        kevent_register(self.kq, events.as_mut_slice())?;
        kevent_check_error(events.as_mut_slice(), &[libc::EPIPE as i64])
    }

    /// De-registers the fd.
    pub fn deregister(&self, fd: i32) -> io::Result<()> {
        let flags = libc::EV_DELETE | libc::EV_RECEIPT;
        let mut events = vec![
            kevent_new(fd, libc::EVFILT_READ, flags, 0),
            kevent_new(fd, libc::EVFILT_WRITE, flags, 0),
        ];
        kevent_register(self.kq, events.as_mut_slice())?;
        kevent_check_error(events.as_mut_slice(), &[libc::ENOENT as i64])
    }

    /// Try-clones the kqueue.
    ///
    /// If succeeds, returns a duplicate of the kqueue.
    /// If fails, returns the last OS error.
    pub fn try_clone(&self) -> io::Result<Selector> {
        const LOWEST_FD: c_int = 3;

        let kq = syscall!(fcntl(self.kq, libc::F_DUPFD_CLOEXEC, LOWEST_FD))?;
        Ok(Selector { kq })
    }

    /// Allows the kqueue to accept user-space notifications. Should be called
    /// before `Selector::wake`
    pub fn register_waker(&self, token: Token) -> io::Result<()> {
        let event = kevent_new(
            0,
            libc::EVFILT_USER,
            libc::EV_ADD | libc::EV_CLEAR | libc::EV_RECEIPT,
            token.0,
        );

        self.kevent_notify(event)
    }

    /// Sends a notification to wakeup the kqueue. Should be called after
    /// `Selector::register_waker`.
    pub fn wake(&self, token: Token) -> io::Result<()> {
        let mut event = kevent_new(
            0,
            libc::EVFILT_USER,
            libc::EV_ADD | libc::EV_RECEIPT,
            token.0,
        );
        event.fflags = libc::NOTE_TRIGGER;
        self.kevent_notify(event)
    }

    #[inline]
    fn kevent_notify(&self, mut event: Event) -> io::Result<()> {
        syscall!(kevent(self.kq, &event, 1, &mut event, 1, ptr::null())).map(|_| {
            if (event.flags & libc::EV_ERROR != 0) && event.data != 0 {
                Err(io::Error::from_raw_os_error(event.data as i32))
            } else {
                Ok(())
            }
        })?
    }
}

#[inline]
fn kevent_register(kq: RawFd, events: &mut [Event]) -> io::Result<()> {
    match syscall!(kevent(
        kq,
        events.as_ptr(),
        events.len() as c_int,
        events.as_mut_ptr(),
        events.len() as c_int,
        ptr::null(),
    )) {
        Ok(_) => Ok(()),
        Err(e) => {
            if let Some(libc::EINTR) = e.raw_os_error() {
                Ok(())
            } else {
                Err(e)
            }
        }
    }
}

// this function should be called right after register
fn kevent_check_error(events: &mut [Event], ignored: &[i64]) -> io::Result<()> {
    for event in events {
        let data = event.data as _;
        if (event.flags & libc::EV_ERROR != 0) && data != 0 && !ignored.contains(&data) {
            return Err(io::Error::from_raw_os_error(data as i32));
        }
    }
    Ok(())
}

#[inline]
fn kevent_new(ident: RawFd, filter: i16, flags: u16, udata: usize) -> Event {
    Event {
        ident: ident as uintptr_t,
        filter,
        flags,
        udata: udata as *mut c_void,
        ..unsafe { mem::zeroed() }
    }
}

/// An io event
pub type Event = libc::kevent;

/// A vector of events
pub struct Events(Vec<Event>);

impl Events {
    /// Initializes a vector of events with an initial capacity
    pub fn with_capacity(capacity: usize) -> Self {
        Events(Vec::with_capacity(capacity))
    }
}

impl Deref for Events {
    type Target = Vec<Event>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for Events {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

// kevent has a member `udata` which has type of `*mut c_void`, therefore it
// does not automatically derive Sync/Send.
unsafe impl Send for Events {}
unsafe impl Sync for Events {}

impl EventTrait for Event {
    fn token(&self) -> Token {
        Token(self.udata as usize)
    }

    fn is_readable(&self) -> bool {
        self.filter == libc::EVFILT_READ || self.filter == libc::EVFILT_USER
    }

    fn is_writable(&self) -> bool {
        self.filter == libc::EVFILT_WRITE
    }

    fn is_read_closed(&self) -> bool {
        self.filter == libc::EVFILT_READ && self.flags & libc::EV_EOF != 0
    }

    fn is_write_closed(&self) -> bool {
        self.filter == libc::EVFILT_WRITE && self.flags & libc::EV_EOF != 0
    }

    fn is_error(&self) -> bool {
        (self.flags & libc::EV_ERROR) != 0 || ((self.flags & libc::EV_EOF) != 0 && self.fflags != 0)
    }
}

impl Drop for Selector {
    fn drop(&mut self) {
        if let Err(e) = syscall!(close(self.kq)) {
            panic!("kqueue release failed: {e}");
        }
    }
}
