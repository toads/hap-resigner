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

use std::task::Waker;

use libc::c_void;

type FfrtTimerHandle = *mut c_void;

pub(crate) struct FfrtTimerEntry(FfrtTimerHandle);

impl FfrtTimerEntry {
    pub(crate) fn timer_register(waker: *mut Waker, dur: u64) -> Self {
        extern "C" fn timer_wake_hook(data: *mut c_void) {
            unsafe {
                let waker = data as *mut Waker;
                (*waker).wake_by_ref();
            }
        }

        let data = waker as *mut c_void;
        unsafe {
            let ptr = ylong_ffrt::ffrt_timer_start(dur, data, timer_wake_hook);
            ylong_ffrt::ffrt_poller_wakeup();
            FfrtTimerEntry(ptr)
        }
    }

    pub(crate) fn result(&self) -> bool {
        unsafe { ylong_ffrt::ffrt_timer_query(self.0) == 1 }
    }

    pub(crate) fn timer_deregister(&self) {
        unsafe {
            ylong_ffrt::ffrt_timer_stop(self.0);
        }
    }
}
