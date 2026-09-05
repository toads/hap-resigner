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

//! The code inside a signal handler should be async-signal-safe, you can check
//! the definition here: <https://man7.org/linux/man-pages/man7/signal-safety.7.html.>
//! For short, a signal can be happened at anytime in a thread and the signal
//! handler will be executed on the same exact thread. Therefore, if the signal
//! handler function needs a resource that has been already acquired by the
//! thread (like a nonreentrant mutex), it could cause deadlock.
//!
//! In this crate, the signal handler needs to read the action of a signal from
//! a global singleton signal-manager. This signal-manager should be protected
//! by a lock to ensure atomicity. However, we could not use the regular
//! [`std::sync::RwLock`] because this lock is not async-signal-safe.
//!
//! Thus, we need to implement a spinning RwLock that provides non-block read
//! method for the signal handler to use.

use std::hint;
use std::marker::PhantomData;
use std::ops::Deref;
use std::ptr::null_mut;
use std::sync::atomic::{AtomicPtr, AtomicUsize, Ordering};
use std::sync::{Mutex, MutexGuard};

const VERSIONS: usize = 2;
const HOLDER_COUNT_MAX: usize = usize::MAX / 2;

pub(crate) struct SpinningRwLock<T> {
    version: AtomicUsize,
    data: [AtomicPtr<T>; VERSIONS],
    version_holder_count: [AtomicUsize; VERSIONS],
    write_lock: Mutex<()>,
    _phantom: PhantomData<T>,
}

impl<T> SpinningRwLock<T> {
    pub(crate) fn new(data: T) -> Self {
        let val = Box::new(data);
        let val_ptr = Box::into_raw(val);

        let datas = [AtomicPtr::new(val_ptr), Default::default()];

        SpinningRwLock {
            data: datas,
            version: Default::default(),
            version_holder_count: Default::default(),
            write_lock: Mutex::new(()),
            _phantom: Default::default(),
        }
    }

    pub(crate) fn read(&self) -> ReadGuard<T> {
        loop {
            let version = self.version.load(Ordering::SeqCst) % VERSIONS;
            let curr_count = &self.version_holder_count[version];

            if curr_count.fetch_add(1, Ordering::SeqCst) > HOLDER_COUNT_MAX {
                // read function is called inside a signal handler, so we cannot return an error
                // or panic directly, instead we use libc::abort
                unsafe { libc::abort() };
            }

            // This data could already be nullptr in the following execution order
            // 1. reader loads the current version
            // 2. writer increments the version
            // 3. writer sets old data to nullptr
            // 4. writer blocking waits until old version counter is 0
            // 5. reader increments the old version counter
            // 6. reader acquires the old data using the old version
            // In this case, reader should try again.
            let data = self.data[version].load(Ordering::SeqCst);
            if data.is_null() {
                curr_count.fetch_sub(1, Ordering::SeqCst);
                continue;
            }
            // this is safe because we just check the data is not nullptr, which means the
            // writer has not yet released this data. The reader adds the holder
            // count before acquire the data, the writer will not release the
            // data until the all readers get dropped.
            let data = unsafe { &*data };

            return ReadGuard {
                data,
                version_holder_count: curr_count,
            };
        }
    }

    pub(crate) fn write(&self) -> WriteGuard<T> {
        let guard = self.write_lock.lock().unwrap();
        let version = self.version.load(Ordering::SeqCst);

        WriteGuard {
            lock: self,
            version,
            _guard: guard,
        }
    }

    pub(crate) fn wait_version_release(&self, version: usize) {
        let count = &self.version_holder_count[version];
        while count.load(Ordering::SeqCst) != 0 {
            hint::spin_loop();
        }
    }
}

pub(crate) struct ReadGuard<'a, T: 'a> {
    pub(crate) data: &'a T,
    version_holder_count: &'a AtomicUsize,
}

impl<'a, T> Drop for ReadGuard<'a, T> {
    fn drop(&mut self) {
        self.version_holder_count.fetch_sub(1, Ordering::SeqCst);
    }
}

impl<'a, T> Deref for ReadGuard<'a, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        self.data
    }
}

pub(crate) struct WriteGuard<'a, T: 'a> {
    lock: &'a SpinningRwLock<T>,
    version: usize,
    _guard: MutexGuard<'a, ()>,
}

impl<'a, T> WriteGuard<'a, T> {
    pub(crate) fn store(&mut self, val: T) {
        let val = Box::new(val);
        let val_ptr = Box::into_raw(val);

        let old_version = self.version % VERSIONS;
        let new_version = (old_version + 1) % VERSIONS;
        self.lock.data[new_version].store(val_ptr, Ordering::SeqCst);
        self.lock.version.store(new_version, Ordering::SeqCst);

        let old_data = self.lock.data[old_version].swap(null_mut(), Ordering::SeqCst);
        self.lock.wait_version_release(old_version);
        self.version = new_version;

        // the old data is valid and currently no one is holding it,
        // therefore the drop is safe
        unsafe {
            drop(Box::from_raw(old_data));
        }
    }
}

impl<'a, T> Deref for WriteGuard<'a, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        let data = self.lock.data[self.version].load(Ordering::SeqCst);
        // the write guard always points to a valid data ptr
        unsafe { &*data }
    }
}
