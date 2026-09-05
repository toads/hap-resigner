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

#![cfg(feature = "sync")]
use std::sync::Arc;

/// SDV test cases for Semaphore Mutex cancel-safe
///
/// # Brief
/// 1. Create a counting auto-release-semaphore with an initial capacity.
/// 2. Asynchronously acquires a permit multiple times.
/// 3. Cancel half of the asynchronous tasks
/// 4. Execute remaining tasks
#[test]
fn sdv_semaphore_cancel_test() {
    let sema = Arc::new(ylong_runtime::sync::AutoRelSemaphore::new(1).unwrap());
    let mut handles = vec![];
    let mut canceled_handles = vec![];
    for i in 0..100 {
        let sema_cpy = sema.clone();
        let handle = ylong_runtime::spawn(async move {
            for _ in 0..1000 {
                let ret = sema_cpy.acquire().await.unwrap();
                drop(ret);
            }
            1
        });
        if i % 2 == 0 {
            handles.push(handle);
        } else {
            canceled_handles.push(handle);
        }
    }
    for handle in canceled_handles {
        handle.cancel();
    }
    for handle in handles {
        let ret = ylong_runtime::block_on(handle).unwrap();
        assert_eq!(ret, 1);
    }
}
