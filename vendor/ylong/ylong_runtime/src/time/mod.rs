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

//! Utilities for tracking time.

mod error;
mod sleep;
mod timeout;
mod timer;

cfg_not_ffrt!(
    mod driver;
    mod wheel;

    pub(crate) use driver::{TimeDriver, TimeHandle};
    pub(crate) use wheel::Clock;
);

pub use sleep::{sleep, sleep_until, Sleep};
pub use timeout::timeout;
pub use timer::{periodic_schedule, timer, timer_at, Timer};
