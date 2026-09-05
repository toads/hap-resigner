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

//! A test for Runtime initialization failure, this should panic

use std::thread;
use std::time::Duration;

fn main() {
    let mut vec = vec![];
    let mut count = 0;
    for _ in 0..16326 {
        count += 1;
        println!("{count}");
        let handle = thread::spawn(move || {
            thread::sleep(Duration::from_secs(1000));
        });
        vec.push(handle);
    }

    println!("start to initialize the runtime");
    let handle = ylong_runtime::spawn(async move {
        println!("runtime initialized");
    });
    ylong_runtime::block_on(handle).unwrap();

    for handle in vec {
        handle.join().unwrap();
    }
}
