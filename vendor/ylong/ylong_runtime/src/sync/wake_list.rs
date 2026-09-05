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

use std::cell::UnsafeCell;
use std::cmp;
use std::hint::spin_loop;
use std::ops::{Deref, DerefMut};
use std::sync::atomic::Ordering::{AcqRel, Acquire};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::task::Waker;

use crate::util::slots::{Slots, SlotsError};

/// The first left most bit represents LOCKED state
const LOCKED: usize = 1 << 0;
/// The third left most bit represents NOTIFIABLE state
const NOTIFIABLE: usize = 1 << 1;

pub(crate) struct Inner {
    wake_list: Slots<ListItem>,
}

pub(crate) struct ListItem {
    // Task waker
    pub(crate) wake: Waker,
    // The status of task to get semaphores
    pub(crate) wait_permit: Arc<AtomicUsize>,
}

/// Lists of Wakers
pub(crate) struct WakerList {
    flag: AtomicUsize,
    inner: UnsafeCell<Inner>,
}

/// Safety: `WakerList` is  not `Sync` and `Send` because of `UnsafeCell`.
/// However, we lock `WakerList` first when we try to access it. So it is safe
/// for `WakerList` to be sent and borrowed across threads.
unsafe impl Sync for WakerList {}
unsafe impl Send for WakerList {}

impl WakerList {
    #[inline]
    pub fn new() -> WakerList {
        WakerList {
            flag: AtomicUsize::new(0),
            inner: UnsafeCell::new(Inner {
                wake_list: Slots::new(),
            }),
        }
    }

    /// Pushes a waker into the list and return its index in the list.
    pub fn insert(&self, waker: ListItem) -> usize {
        let mut list = self.lock();

        list.wake_list.push_back(waker)
    }

    /// Removes the waker corresponding to the key.
    #[allow(dead_code)]
    pub fn remove(&self, key: usize) -> Result<ListItem, SlotsError> {
        let mut inner = self.lock();
        inner.wake_list.remove(key)
    }

    /// Wakes up one more member, no matter whether someone is being waking up
    /// at the same time. This method is an atomic operation. If a
    /// non-atomic operation is required, call `lock` first and then call
    /// `notify_one`.
    #[inline]
    pub fn notify_one(&self) -> bool {
        self.notify(Notify::One)
    }

    /// Wakes up all members in the WakerList, and return the result.
    /// This method is an atomic operation. If a non-atomic operation is
    /// required, call `lock` first and then call `notify_all`.
    #[inline]
    pub fn notify_all(&self) -> bool {
        self.notify(Notify::All)
    }

    fn notify(&self, notify_type: Notify) -> bool {
        if self.flag.load(Ordering::SeqCst) & NOTIFIABLE != 0 {
            let mut inner = self.lock();
            inner.notify(notify_type)
        } else {
            false
        }
    }

    /// Locks up the WakerList. If it has been already locked, spin loop until
    /// fetch the lock.
    pub fn lock(&self) -> Lock<'_> {
        // This condition will be false only if the flag is LOCKED.
        while self.flag.fetch_or(LOCKED, Ordering::Acquire) & LOCKED != 0 {
            spin_loop();
        }
        Lock { waker_set: self }
    }
}

impl ListItem {
    fn get_wait_permit(&self) -> usize {
        self.wait_permit.load(Acquire)
    }
    fn change_permit(&self, curr: usize, next: usize) -> Result<usize, usize> {
        self.wait_permit.
            compare_exchange(curr, next, AcqRel, Acquire)
    }

    fn change_status(&self, acquired_permit: usize) -> bool {
        let mut curr = self.get_wait_permit();
        loop {
            let assign = cmp::min(curr, acquired_permit);
            let next = curr - assign;
            match self.change_permit(curr, next) {
                Ok(_) => return next == 0,
                Err(actual) => curr = actual,
            }
        }
    }
}

impl Inner {
    /// Wakes up one or more members in the WakerList, and return the result.
    #[inline]
    fn notify(&mut self, notify_type: Notify) -> bool {
        let mut is_wake = false;
        while let Some(list_item) = self.wake_list.get_first() {
            let res= list_item.change_status(1);
            if res {
                // If entering this branch, 'wake_list.pop_front()' must be 'Some(_)'
                let pop = self.wake_list.pop_front().expect("The list first is NULL");
                pop.wake.wake();
                is_wake = true;
            }
            if notify_type == Notify::One {
                return is_wake;
            }
        }
        is_wake
    }

    /// Wakes up one more member, no matter whether someone is being waking up
    /// at the same time.
    #[inline]
    pub fn notify_one(&mut self) -> bool {
        self.notify(Notify::One)
    }

    /// Wakes up all members in the WakerList, and return the result.
    #[inline]
    pub fn notify_all(&mut self) -> bool {
        self.notify(Notify::All)
    }
}

/// The guard holding the WakerList.
pub(crate) struct Lock<'a> {
    waker_set: &'a WakerList,
}

impl Lock<'_> {
    pub(crate) fn remove_permit(&mut self, key: usize, wait_permit: usize) -> bool {
        if let Some(list_item) = self.wake_list.get_by_index(key) {
            let inner_wait_permit = list_item.get_wait_permit();
            if inner_wait_permit == wait_permit {
                let _ = self.wake_list.remove(key);
            }
        }
        if wait_permit == 0 {
            return !self.notify_one();
        }
        false
    }
}

impl Drop for Lock<'_> {
    #[inline]
    fn drop(&mut self) {
        let mut flag = 0;
        // If there're members that can be notified, set the third left most bit, which
        // means to add NOTIFIABLE state to the flag.
        if !self.wake_list.is_empty() {
            flag |= NOTIFIABLE;
        }
        self.waker_set.flag.store(flag, Ordering::SeqCst);
    }
}

impl Deref for Lock<'_> {
    type Target = Inner;

    fn deref(&self) -> &Inner {
        unsafe { &*self.waker_set.inner.get() }
    }
}

impl DerefMut for Lock<'_> {
    fn deref_mut(&mut self) -> &mut Inner {
        unsafe { &mut *self.waker_set.inner.get() }
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum Notify {
    // Wake up one more member based on the current state
    One,
    // Wake up all members
    All,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// UT test cases for WakeList::new().
    ///
    /// # Brief
    /// 1. Check the initial value of flag.
    /// 2. Check the initial value of waiting_number.
    #[test]
    fn ut_wakelist_new_01() {
        let wakelist = WakerList::new();
        assert_eq!(wakelist.flag.load(Ordering::SeqCst), 0);
        unsafe {
            assert_eq!((*wakelist.inner.get()).wake_list.len, 0);
        }
    }
}
