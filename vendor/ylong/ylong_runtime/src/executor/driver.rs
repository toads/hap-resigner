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

use std::sync::{Arc, Mutex};
use std::time::Duration;

pub(crate) use crate::executor::driver_handle::Handle;

cfg_time! {
    use crate::time::TimeDriver;
}

cfg_net! {
    use crate::net::IoDriver;
}

#[cfg(target_family = "unix")]
cfg_signal! {
    use crate::signal::unix::SignalDriver;
}

// Flag used to identify whether to park on condvar.
pub(crate) enum ParkFlag {
    NotPark,
    Park,
    ParkTimeout(Duration),
}

pub(crate) struct Driver {
    #[cfg(feature = "net")]
    io: IoDriver,
    #[cfg(all(feature = "signal", target_family = "unix"))]
    signal: SignalDriver,
    #[cfg(feature = "time")]
    time: Arc<TimeDriver>,
}

impl Driver {
    pub(crate) fn initialize() -> (Arc<Handle>, Arc<Mutex<Driver>>) {
        #[cfg(feature = "net")]
        let (io_handle, io_driver) = IoDriver::initialize();
        #[cfg(feature = "time")]
        let (time_handle, time_driver) = TimeDriver::initialize();
        let handle = Handle {
            #[cfg(feature = "net")]
            io: io_handle,
            #[cfg(feature = "time")]
            time: time_handle,
        };
        #[cfg(all(feature = "signal", target_family = "unix"))]
        let signal_driver = SignalDriver::initialize(&handle);
        let driver = Driver {
            #[cfg(feature = "net")]
            io: io_driver,
            #[cfg(all(feature = "signal", target_family = "unix"))]
            signal: signal_driver,
            #[cfg(feature = "time")]
            time: time_driver,
        };
        (Arc::new(handle), Arc::new(Mutex::new(driver)))
    }

    pub(crate) fn run(&mut self) -> ParkFlag {
        let _duration: Option<Duration> = None;
        #[cfg(feature = "time")]
        let _duration = self.time.run();
        #[cfg(feature = "net")]
        self.io
            .drive(_duration)
            .unwrap_or_else(|e| panic!("io driver running failed, error: {e}"));
        #[cfg(all(feature = "signal", target_family = "unix"))]
        if self.io.process_signal() {
            self.signal.broadcast();
        }
        #[cfg(all(target_os = "linux", feature = "process"))]
        crate::process::GlobalZombieChild::get_instance().release_zombie();
        if cfg!(feature = "net") {
            ParkFlag::NotPark
        } else {
            match _duration {
                None => ParkFlag::Park,
                Some(duration) if duration.is_zero() => ParkFlag::NotPark,
                Some(duration) => ParkFlag::ParkTimeout(duration),
            }
        }
    }

    pub(crate) fn run_once(&mut self) {
        #[cfg(feature = "time")]
        self.time.run();
        #[cfg(feature = "net")]
        self.io
            .drive(Some(Duration::from_millis(0)))
            .unwrap_or_else(|e| panic!("io driver running failed, error: {e}"));
        #[cfg(all(feature = "signal", target_family = "unix"))]
        if self.io.process_signal() {
            self.signal.broadcast();
        }
        #[cfg(all(target_os = "linux", feature = "process"))]
        crate::process::GlobalZombieChild::get_instance().release_zombie();
    }
}
