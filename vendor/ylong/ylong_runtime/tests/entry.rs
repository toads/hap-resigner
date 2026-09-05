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

// cfg gn_test is used to isolate the test compiling on OHOS
#![allow(unexpected_cfgs)]
#![cfg(gn_test)]

mod async_buf_read;
mod async_buf_write;
mod async_dir;
mod async_fs;
mod async_pool;
mod async_read;
mod block_on;
mod builder;
mod cancel_safe;
mod error;
mod join_set;
mod mpsc_test;
mod mutex;
mod par_iter;
#[cfg(feature = "process")]
mod process;
#[cfg(feature = "process")]
mod pty_process;
mod select;
mod semaphore_test;
mod signal;
mod singleton_runtime;
mod spawn;
mod spawn_blocking;
mod sync;
mod task_cancel;
mod tcp_test;
mod timer_test;
mod udp_test;
mod uds_test;
