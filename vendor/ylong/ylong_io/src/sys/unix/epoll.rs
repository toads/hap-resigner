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
use std::os::raw::c_int;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use crate::{EventTrait, Interest, Token};

static NEXT_ID: AtomicUsize = AtomicUsize::new(1);

/// An wrapper for different OS polling system.
/// Linux: epoll
/// Windows: iocp
/// macos: kqueue
pub struct Selector {
    // selector id
    id: usize,
    // epoll fd
    ep: i32,
}

impl Selector {
    /// Creates a new Selector.
    ///
    /// # Error
    /// If the underlying syscall fails, returns the corresponding error.
    pub fn new() -> io::Result<Selector> {
        let ep = match syscall!(epoll_create1(libc::EPOLL_CLOEXEC)) {
            Ok(ep_sys) => ep_sys,
            Err(err) => {
                return Err(err);
            }
        };

        Ok(Selector {
            id: NEXT_ID.fetch_add(1, Ordering::Relaxed),
            ep,
        })
    }

    /// Waits for io events to come within a time limit.
    pub fn select(&self, events: &mut Events, timeout: Option<Duration>) -> io::Result<()> {
        // Convert to milliseconds, if input time is none, it means the timeout is -1
        // and wait permanently.
        let timeout = timeout.map(|time| time.as_millis() as c_int).unwrap_or(-1);

        events.clear();

        match syscall!(epoll_wait(
            self.ep,
            events.as_mut_ptr(),
            events.capacity() as i32,
            timeout
        )) {
            Ok(n_events) => unsafe { events.set_len(n_events as usize) },
            Err(err) => {
                return Err(err);
            }
        }
        Ok(())
    }

    /// Registers the fd with specific interested events
    pub fn register(&self, fd: i32, token: Token, interests: Interest) -> io::Result<()> {
        let mut sys_event = libc::epoll_event {
            events: interests.into_io_event(),
            u64: usize::from(token) as u64,
        };

        match syscall!(epoll_ctl(self.ep, libc::EPOLL_CTL_ADD, fd, &mut sys_event)) {
            Ok(_) => Ok(()),
            Err(err) => Err(err),
        }
    }

    /// Re-registers the fd with specific interested events
    pub fn reregister(&self, fd: i32, token: Token, interests: Interest) -> io::Result<()> {
        let mut sys_event = libc::epoll_event {
            events: interests.into_io_event(),
            u64: usize::from(token) as u64,
        };

        match syscall!(epoll_ctl(self.ep, libc::EPOLL_CTL_MOD, fd, &mut sys_event)) {
            Ok(_) => Ok(()),
            Err(err) => Err(err),
        }
    }

    /// De-registers the fd.
    pub fn deregister(&self, fd: i32) -> io::Result<()> {
        match syscall!(epoll_ctl(
            self.ep,
            libc::EPOLL_CTL_DEL,
            fd,
            std::ptr::null_mut()
        )) {
            Ok(_) => Ok(()),
            Err(err) => Err(err),
        }
    }
}

impl Drop for Selector {
    fn drop(&mut self) {
        if let Err(_err) = syscall!(close(self.ep)) {
            // todo: log the error
        }
    }
}

impl std::fmt::Debug for Selector {
    fn fmt(&self, fmt: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(fmt, "epoll fd: {}, Select id: {}", self.ep, self.id)
    }
}

/// A vector of events
pub type Events = Vec<Event>;

/// An io event
pub type Event = libc::epoll_event;

impl EventTrait for Event {
    fn token(&self) -> Token {
        Token(self.u64 as usize)
    }

    fn is_readable(&self) -> bool {
        (self.events as libc::c_int & libc::EPOLLIN) != 0
            || (self.events as libc::c_int & libc::EPOLLPRI) != 0
    }

    fn is_writable(&self) -> bool {
        (self.events as libc::c_int & libc::EPOLLOUT) != 0
    }

    fn is_read_closed(&self) -> bool {
        self.events as libc::c_int & libc::EPOLLHUP != 0
            || (self.events as libc::c_int & libc::EPOLLIN != 0
                && self.events as libc::c_int & libc::EPOLLRDHUP != 0)
    }

    fn is_write_closed(&self) -> bool {
        self.events as libc::c_int & libc::EPOLLHUP != 0
            || (self.events as libc::c_int & libc::EPOLLOUT != 0
                && self.events as libc::c_int & libc::EPOLLERR != 0)
            || self.events as libc::c_int == libc::EPOLLERR
    }

    fn is_error(&self) -> bool {
        (self.events as libc::c_int & libc::EPOLLERR) != 0
    }
}

#[cfg(test)]
mod test {
    use crate::sys::socket;
    use crate::{Event, EventTrait, Interest, Selector, Token};

    /// UT cases for `Selector::reregister`.
    ///
    /// # Brief
    /// 1. Create a Selector
    /// 2. Reregister the selector
    #[test]
    fn ut_epoll_reregister() {
        let selector = Selector::new().unwrap();
        let sock = socket::socket_new(libc::AF_UNIX, libc::SOCK_STREAM).unwrap();
        let ret = selector.register(sock, Token::from_usize(0), Interest::READABLE);
        assert!(ret.is_ok());
        let ret = selector.reregister(sock, Token::from_usize(0), Interest::WRITABLE);
        assert!(ret.is_ok());
    }

    /// UT case for `Event::is_error`
    ///
    /// # Brief
    /// 1. Create an event from libc::EPOLLERR
    /// 2. Check if it's an error
    #[test]
    fn ut_event_is_err() {
        let event = Event {
            events: libc::EPOLLERR as u32,
            u64: 0,
        };
        assert!(event.is_error());
    }
}
