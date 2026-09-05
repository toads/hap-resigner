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

//! This example simulates the situation of spawn failure due to insufficient
//! memory

use std::time::Duration;

fn main() {
    // loop until the program gets killed automatically
    loop {
        let _handle = ylong_runtime::spawn(async move {
            let buf = vec![0; 2000000];
            ylong_runtime::time::sleep(Duration::from_secs(100)).await;
            assert_eq!(buf, [0; 2000000]);
        });
    }
}
