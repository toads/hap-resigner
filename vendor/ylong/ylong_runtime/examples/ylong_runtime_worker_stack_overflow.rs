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

//! This example simulates the scenario where a worker thread gets its stack
//! overflowed

fn main() {
    ylong_runtime::builder::RuntimeBuilder::new_multi_thread()
        .worker_stack_size(1)
        .build_global()
        .unwrap();

    let handle = ylong_runtime::spawn(async move {
        let a = [0; 20000];
        assert_eq!(a, [0; 20000]);
    });

    ylong_runtime::block_on(handle).unwrap();
}
