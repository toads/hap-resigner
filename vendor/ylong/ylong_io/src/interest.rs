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

use std::num::NonZeroU8;

/// The interested events, such as readable, writeable.
#[derive(Copy, PartialEq, Eq, Clone, PartialOrd, Ord)]
pub struct Interest(NonZeroU8);
use std::ops;

const READABLE: u8 = 0b0001;
const WRITABLE: u8 = 0b0010;

/// A wrapper that wraps around fd events
impl Interest {
    /// An interest for readable events
    pub const READABLE: Interest = Interest(unsafe { NonZeroU8::new_unchecked(READABLE) });
    /// An interest for writeable events
    pub const WRITABLE: Interest = Interest(unsafe { NonZeroU8::new_unchecked(WRITABLE) });

    /// Combines two Interest into one.
    pub const fn add(self, other: Interest) -> Interest {
        Interest(unsafe { NonZeroU8::new_unchecked(self.0.get() | other.0.get()) })
    }

    /// Checks if the interest is for readable events.
    pub const fn is_readable(self) -> bool {
        (self.0.get() & READABLE) != 0
    }

    /// Checks if the interest is for writeable events.
    pub const fn is_writable(self) -> bool {
        (self.0.get() & WRITABLE) != 0
    }

    /// Convert interest to the event value.
    #[cfg(target_os = "linux")]
    pub fn into_io_event(self) -> libc::c_uint {
        let mut io_event = libc::EPOLLET as u32;

        if self.is_readable() {
            io_event |= libc::EPOLLIN as u32;
            io_event |= libc::EPOLLRDHUP as u32;
        }

        if self.is_writable() {
            io_event |= libc::EPOLLOUT as u32;
        }

        io_event as libc::c_uint
    }
}

impl ops::BitOr for Interest {
    type Output = Self;

    #[inline]
    fn bitor(self, other: Self) -> Self {
        self.add(other)
    }
}

#[cfg(test)]
mod test {
    /// UT cases for `into_io_event`.
    ///
    /// # Brief
    /// 1. Create different kinds of Interest
    /// 2. Turn the Interest into IO Event
    #[cfg(target_os = "linux")]
    #[test]
    fn ut_interest_to_io_event() {
        use std::num::NonZeroU8;

        use libc::c_int;

        use crate::Interest;

        #[allow(clippy::init_numbered_fields)]
        let interest = Interest {
            0: NonZeroU8::new(4).unwrap(),
        };
        let event = interest.into_io_event();
        assert_eq!(event as c_int, libc::EPOLLET);

        let interest = Interest::READABLE;
        let event = interest.into_io_event();
        assert_eq!(
            event as c_int,
            libc::EPOLLET | libc::EPOLLIN | libc::EPOLLRDHUP
        );

        let interest = Interest::WRITABLE;
        let event = interest.into_io_event();
        assert_eq!(event as c_int, libc::EPOLLET | libc::EPOLLOUT);

        let interest = Interest::READABLE | Interest::WRITABLE;
        let event = interest.into_io_event();
        assert_eq!(
            event as c_int,
            libc::EPOLLET | libc::EPOLLIN | libc::EPOLLRDHUP | libc::EPOLLOUT
        );
    }
}
