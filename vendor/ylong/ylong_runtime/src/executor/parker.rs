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

use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering::SeqCst;
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::Duration;

use crate::executor::driver::{Driver, Handle, ParkFlag};

#[derive(Clone)]
pub(crate) struct Parker {
    inner: Arc<Inner>,
}

struct Inner {
    state: AtomicUsize,
    mutex: Mutex<()>,
    condvar: Condvar,
    driver: Arc<Mutex<Driver>>,
}

const IDLE: usize = 0;
const PARKED_ON_CONDVAR: usize = 1;
const PARKED_ON_DRIVER: usize = 2;
const NOTIFIED: usize = 3;

impl Parker {
    pub(crate) fn new(driver: Arc<Mutex<Driver>>) -> Parker {
        Parker {
            inner: Arc::new(Inner {
                state: AtomicUsize::new(IDLE),
                mutex: Mutex::new(()),
                condvar: Condvar::new(),
                driver,
            }),
        }
    }

    pub(crate) fn park(&mut self) {
        self.inner.park()
    }

    pub(crate) fn unpark(&self, handle: Arc<Handle>) {
        self.inner.unpark(handle);
    }

    pub(crate) fn get_driver(&self) -> &Arc<Mutex<Driver>> {
        self.inner.get_driver()
    }

    pub(crate) fn release(&self) {
        self.inner.release();
    }

    #[cfg(feature = "metrics")]
    pub(crate) fn get_state(&self) -> usize {
        self.inner.get_state()
    }
}

impl Inner {
    fn park(&self) {
        // loop to reduce the chance of parking the thread
        for _ in 0..3 {
            if self
                .state
                .compare_exchange(NOTIFIED, IDLE, SeqCst, SeqCst)
                .is_ok()
            {
                return;
            }
            thread::yield_now();
        }

        let mut park_flag = ParkFlag::Park;
        if let Ok(mut driver) = self.driver.try_lock() {
            park_flag = self.park_on_driver(&mut driver);
        }

        match park_flag {
            ParkFlag::NotPark => {}
            ParkFlag::Park => self.park_on_condvar_timeout(None),
            ParkFlag::ParkTimeout(duration) => self.park_on_condvar_timeout(Some(duration)),
        }
    }

    fn park_on_driver(&self, driver: &mut Driver) -> ParkFlag {
        match self
            .state
            .compare_exchange(IDLE, PARKED_ON_DRIVER, SeqCst, SeqCst)
        {
            Ok(_) => {}
            Err(NOTIFIED) => {
                self.state.swap(IDLE, SeqCst);
                return ParkFlag::NotPark;
            }
            Err(actual) => panic!("inconsistent park state; actual = {actual}"),
        }

        let park_flag = driver.run();

        match self.state.swap(IDLE, SeqCst) {
            // got notified by real io events or not
            NOTIFIED => ParkFlag::NotPark,
            PARKED_ON_DRIVER => park_flag,
            n => panic!("inconsistent park_timeout state: {n}"),
        }
    }

    // if duration is none, than park permanently
    fn park_on_condvar_timeout(&self, duration: Option<Duration>) {
        let mut l = self.mutex.lock().unwrap();
        match self
            .state
            .compare_exchange(IDLE, PARKED_ON_CONDVAR, SeqCst, SeqCst)
        {
            Ok(_) => {}
            Err(NOTIFIED) => {
                // got a notification, exit parking
                self.state.swap(IDLE, SeqCst);
                return;
            }
            Err(actual) => panic!("inconsistent park state; actual = {actual}"),
        }

        loop {
            let mut is_timed_out = false;
            if let Some(duration) = duration {
                let (lock, timeout_result) = self.condvar.wait_timeout(l, duration).unwrap();
                is_timed_out = timeout_result.timed_out();
                l = lock;
            } else {
                l = self.condvar.wait(l).unwrap();
            }

            if self
                .state
                .compare_exchange(NOTIFIED, IDLE, SeqCst, SeqCst)
                .is_ok()
            {
                // got a notification, finish parking
                return;
            }

            if is_timed_out {
                self.state.store(IDLE, SeqCst);
                return;
            }
            // got spurious wakeup, go back to park again
        }
    }

    fn unpark(&self, handle: Arc<Handle>) {
        match self.state.swap(NOTIFIED, SeqCst) {
            IDLE | NOTIFIED => {}
            PARKED_ON_CONDVAR => {
                drop(self.mutex.lock());
                self.condvar.notify_one();
            }
            PARKED_ON_DRIVER => handle.wake(),
            actual => panic!("inconsistent state in unpark; actual = {actual}"),
        }
    }

    pub(crate) fn get_driver(&self) -> &Arc<Mutex<Driver>> {
        &self.driver
    }

    #[cfg(feature = "metrics")]
    pub(crate) fn get_state(&self) -> usize {
        self.state.load(SeqCst)
    }

    fn release(&self) {
        self.condvar.notify_all();
    }
}
