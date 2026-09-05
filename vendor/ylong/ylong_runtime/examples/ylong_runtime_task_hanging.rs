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

//! A test for task hanging

use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

fn main() {
    let resource_one = Arc::new(Mutex::new(1));
    let resource_two = Arc::new(Mutex::new(2));
    let r_one = resource_one.clone();
    let r_two = resource_two.clone();
    let start = Instant::now();
    let handle_one = ylong_runtime::spawn(async move {
        let binding = resource_one.clone();
        let lock_one = binding.lock();
        thread::sleep(Duration::from_millis(100));
        drop(lock_one);
        let binding = resource_two.clone();
        let _lock_two = binding.lock();
        1
    });
    let handle_two = ylong_runtime::spawn(async move {
        let binding = r_two.clone();
        let lock_two = binding.lock();
        thread::sleep(Duration::from_millis(100));
        drop(lock_two);
        let binding = r_one.clone();
        let _lock_one = binding.lock();
        2
    });

    let one = ylong_runtime::block_on(handle_one).unwrap();
    let two = ylong_runtime::block_on(handle_two).unwrap();
    let dur = start.elapsed().as_millis();
    assert_eq!(one, 1);
    assert_eq!(two, 2);
    assert!(dur >= 100, "the duration should more than 100");
}
