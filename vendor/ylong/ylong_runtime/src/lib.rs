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

#![warn(missing_docs)]

//! # ylong_runtime
//! A runtime for writing IO-bounded and CPU-bounded applications.

#[cfg(all(
    feature = "ffrt",
    any(feature = "current_thread_runtime", feature = "multi_instance_runtime")
))]
compile_error!("Feature ffrt can not be enabled with feature current_thread_runtime or feature multi_instance_runtime");

#[cfg(all(feature = "ffrt", not(target_os = "linux")))]
compile_error!("Feature ffrt only works on linux currently");

#[cfg(all(feature = "ffrt", feature = "metrics"))]
compile_error!("Feature ffrt can not be enabled with feature metrics");

extern crate core;

#[macro_use]
mod macros;

pub mod builder;
pub mod error;
pub mod executor;
pub mod fastrand;
pub mod futures;
pub mod io;
pub mod iter;
pub mod task;

pub use crate::task::{block_on, spawn, spawn_blocking};

mod spawn;
mod util;

cfg_ffrt! {
    pub(crate) mod ffrt;
    pub use ylong_ffrt::Qos;
}

cfg_macros! {
    mod select;
    pub use ylong_runtime_macros::tuple_form;
}

cfg_time! {
    pub mod time;
}

cfg_signal! {
    pub mod signal;
}

cfg_sync! {
    pub mod sync;
}

cfg_metrics! {
    mod metrics;
    pub use metrics::Metrics;
}

cfg_fs! {
    pub mod fs;
}

cfg_net! {
    pub mod net;
}

#[cfg(target_os = "linux")]
cfg_process! {
    pub mod process;
}
