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

//! Benchmarks for the tcp.

#![feature(test)]

extern crate core;

mod task_helpers;

#[macro_export]
macro_rules! tokio_task_creation_global {
    ($runtime: expr, $bench: ident, $task_num: literal) => {
        #[bench]
        fn $bench(b: &mut Bencher) {
            let runtime = $runtime;
            b.iter(black_box(|| {
                let mut handlers = Vec::new();
                for _ in 0..$task_num {
                    handlers.push(runtime.spawn(async move { 1 }));
                }
            }));
        }
    };
}

#[macro_export]
macro_rules! ylong_task_creation_global {
    ($bench: ident, $task_num: literal) => {
        #[bench]
        fn $bench(b: &mut Bencher) {
            ylong_runtime_init();
            b.iter(black_box(|| {
                let mut handlers = Vec::new();
                for _ in 0..$task_num {
                    handlers.push(ylong_runtime::spawn(async move { 1 }));
                }
            }));
        }
    };
}

#[macro_export]
macro_rules! tokio_task_creation_local {
    ($runtime: expr, $bench: ident, $task_num: literal) => {
        #[bench]
        fn $bench(b: &mut Bencher) {
            let runtime = $runtime;
            b.iter(black_box(|| {
                let handle = runtime.spawn(async move {
                    let mut handlers = Vec::new();
                    for _ in 0..$task_num {
                        handlers.push(tokio::spawn(async move { 1 }));
                    }
                });
                runtime.block_on(handle).unwrap();
            }));
        }
    };
}

#[macro_export]
macro_rules! ylong_task_creation_local {
    ($bench: ident, $task_num: literal) => {
        #[bench]
        fn $bench(b: &mut Bencher) {
            ylong_runtime_init();
            b.iter(black_box(|| {
                let handle = ylong_runtime::spawn(async move {
                    let mut handlers = Vec::new();
                    for _ in 0..$task_num {
                        handlers.push(ylong_runtime::spawn(async move { 1 }));
                    }
                });
                ylong_runtime::block_on(handle).unwrap();
            }));
        }
    };
}

#[cfg(test)]
mod task_creation {
    extern crate test;

    use std::hint::black_box;

    use test::Bencher;

    use crate::task_helpers::{tokio_runtime, ylong_runtime_init};

    ylong_task_creation_global!(ylong_task_10_global, 10);
    ylong_task_creation_global!(ylong_task_100_global, 100);
    ylong_task_creation_global!(ylong_task_1000_global, 1000);

    tokio_task_creation_global!(tokio_runtime(), tokio_task_10_global, 10);
    tokio_task_creation_global!(tokio_runtime(), tokio_task_100_global, 100);
    tokio_task_creation_global!(tokio_runtime(), tokio_task_1000_global, 1000);

    ylong_task_creation_local!(ylong_task_10_local, 10);
    ylong_task_creation_local!(ylong_task_100_local, 100);
    ylong_task_creation_local!(ylong_task_1000_local, 1000);

    tokio_task_creation_local!(tokio_runtime(), tokio_task_10_local, 10);
    tokio_task_creation_local!(tokio_runtime(), tokio_task_100_local, 100);
    tokio_task_creation_local!(tokio_runtime(), tokio_task_1000_local, 1000);
}
