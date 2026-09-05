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

//! Sleep latency in ylong_runtime.

use std::time::{Duration, Instant};

fn main() {
    let mut handlers = vec![];
    for _ in 0..1000 {
        let handle = ylong_runtime::spawn(async move {
            let duration = Duration::from_millis(100);
            let start = Instant::now();
            ylong_runtime::time::sleep(duration).await;
            let since = start.elapsed();
            let latency = since.saturating_sub(duration).as_millis();
            println!("since is {}", since.as_millis());
            latency
        });
        handlers.push(handle);
    }
    let mut average = 0;
    for handler in handlers {
        let time = ylong_runtime::block_on(handler).unwrap();
        average += time;
    }
    average /= 1000;
    println!("ylong average latency is {} millisecond", average);

    let runtime = tokio::runtime::Runtime::new().unwrap();
    let mut handlers = vec![];
    for _ in 0..1000 {
        let handle = runtime.spawn(async move {
            let duration = Duration::from_millis(100);
            let start = Instant::now();
            tokio::time::sleep(duration).await;
            let since = start.elapsed();
            let latency = since.saturating_sub(duration).as_millis();
            println!("since is {}", since.as_millis());
            latency
        });
        handlers.push(handle);
    }
    let mut average = 0;
    for handler in handlers {
        let time = runtime.block_on(handler).unwrap();
        average += time;
    }
    average /= 1000;
    println!("average latency is {} millisecond", average);
}
