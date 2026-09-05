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

use std::collections::HashMap;
use std::mem::MaybeUninit;
use std::sync::Once;

use libc::c_int;

use crate::common::{SigAction, Signal};
use crate::spin_rwlock::SpinningRwLock;

pub(crate) struct SigMap {
    pub(crate) data: SpinningRwLock<HashMap<c_int, Signal>>,
    pub(crate) race_old: SpinningRwLock<Option<SigAction>>,
}

impl SigMap {
    // This function will be called inside a signal handler.
    // Although a mutex Once is used, but the mutex will only be locked for once
    // when initializing the SignalManager, which is outside of the signal
    // handler.
    pub(crate) fn get_instance() -> &'static SigMap {
        static ONCE: Once = Once::new();
        static mut GLOBAL_SIG_MAP: MaybeUninit<SigMap> = MaybeUninit::uninit();

        unsafe {
            ONCE.call_once(|| {
                GLOBAL_SIG_MAP = MaybeUninit::new(SigMap {
                    data: SpinningRwLock::new(HashMap::new()),
                    race_old: SpinningRwLock::new(None),
                });
            });
            GLOBAL_SIG_MAP.assume_init_ref()
        }
    }
}
