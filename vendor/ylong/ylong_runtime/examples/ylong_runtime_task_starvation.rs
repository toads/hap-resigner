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

//! This example simulates the situation of task starvation by spawning long
//! tasks that have no await point

use std::thread;
use std::time::Duration;

use tokio::time::Instant;

fn main() {
    // initialize the runtime with only 1 worker thread
    ylong_runtime::builder::RuntimeBuilder::new_multi_thread()
        .worker_num(1)
        .build_global()
        .unwrap();

    let instant = Instant::now();
    let _long_hold = ylong_runtime::spawn(async move {
        thread::sleep(Duration::from_secs(5));
    });
    let handle = ylong_runtime::spawn(async move {
        let a = 0;
        assert_eq!(a, 0);
    });
    ylong_runtime::block_on(handle).unwrap();
    let time_cost = instant.elapsed().as_secs();
    println!("time cost: {}", time_cost);
    assert!(time_cost >= 5);
}
