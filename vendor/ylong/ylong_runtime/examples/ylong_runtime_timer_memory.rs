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

use std::time::Duration;

use ylong_runtime::block_on;
use ylong_runtime::builder::RuntimeBuilder;

const DATA_SIZE: usize = 4 * 1024 * 1024;

async fn read_big_data() -> Vec<u8> {
    let data = vec![1_u8; DATA_SIZE];
    ylong_runtime::time::sleep(Duration::from_secs_f32(0.5)).await;
    ylong_runtime::time::sleep(Duration::from_secs_f32(30.0)).await;
    data
}

async fn run_forever() {
    loop {
        ylong_runtime::spawn(async {
            if let Ok(v) =
                ylong_runtime::time::timeout(Duration::from_secs(1), read_big_data()).await
            {
                if v.len() < DATA_SIZE {
                    println!("length: {}", v.len())
                }
            }
        });
        ylong_runtime::time::sleep(Duration::from_secs_f32(0.1)).await;
    }
}

fn main() {
    RuntimeBuilder::new_multi_thread()
        .worker_num(8)
        .build_global()
        .unwrap();
    block_on(async {
        for _ in 0..8 {
            ylong_runtime::spawn(async { run_forever().await });
        }
    });
    std::thread::sleep(Duration::from_secs(3600));
}
