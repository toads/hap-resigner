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

use std::sync::atomic::Ordering::{Acquire, Release};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

#[cfg(feature = "ffrt")]
use ylong_ffrt::Qos;
use ylong_runtime::builder::RuntimeBuilder;
use ylong_runtime::task::yield_now;

struct Parker {
    parker: AtomicUsize,
}

impl Parker {
    fn new(size: usize) -> Parker {
        Parker {
            parker: AtomicUsize::new(size),
        }
    }

    fn wait_for_all(&self) {
        while self.parker.load(Acquire) != 0 {}
    }

    fn wake_one(&self) {
        self.parker.fetch_sub(1, Release);
    }
}

/// SDV case for yield_now.
///
/// # Brief
/// 1. Configures the runtime to have single worker
/// 2. Starts 10 tasks, each fetch-adds an atomic usize and then yields for 100
///    times
/// 3. At the end of each task, checks if the atomic value is greater than 100.
///    If greater than 100, it means that another task had been executed, yield
///    works.
#[test]
fn sdv_yield_now_single_worker() {
    #[cfg(feature = "ffrt")]
    RuntimeBuilder::new_multi_thread()
        .max_worker_num_by_qos(Qos::Default, 1)
        .build_global()
        .unwrap();

    #[cfg(not(feature = "ffrt"))]
    RuntimeBuilder::new_multi_thread()
        .worker_num(1)
        .build_global()
        .unwrap();

    let val = Arc::new(AtomicUsize::new(0));
    let parker = Arc::new(Parker::new(10));
    let mut handles = vec![];
    for _ in 0..10 {
        let val_cpy = val.clone();
        let parker_cpy = parker.clone();
        let handle = ylong_runtime::spawn(async move {
            parker_cpy.wait_for_all();
            for _ in 0..100 {
                val_cpy.fetch_add(1, Ordering::Relaxed);
                yield_now().await;
            }
            let cur = val_cpy.load(Ordering::Relaxed);
            assert!(cur > 100);
        });
        handles.push(handle);
        parker.wake_one();
    }
    for handle in handles {
        ylong_runtime::block_on(handle).unwrap();
    }
}
