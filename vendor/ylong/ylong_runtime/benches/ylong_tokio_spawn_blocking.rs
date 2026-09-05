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

//! Benchmarks for spawn_blocking.

#![feature(test)]
extern crate core;

mod task_helpers;

extern crate test;

#[macro_export]
macro_rules! tokio_spawn_blocking_task {
    ($runtime: expr, $bench: ident, $num: literal, $upper: literal) => {
        #[bench]
        fn $bench(b: &mut Bencher) {
            let runtime = $runtime;
            b.iter(black_box(|| {
                let mut handlers = Vec::with_capacity($num);
                for _ in 0..$num {
                    handlers.push(runtime.spawn_blocking(|| {
                        fibbo($upper);
                    }));
                }

                for handler in handlers {
                    let _ = runtime.block_on(handler).unwrap();
                }
            }));
        }
    };
}

#[macro_export]
macro_rules! ylong_spawn_blocking_task {
    ($bench: ident, $num: literal, $upper: literal) => {
        #[bench]
        fn $bench(b: &mut Bencher) {
            b.iter(black_box(|| {
                let mut handlers = Vec::with_capacity($num);
                for _ in 0..$num {
                    handlers.push(ylong_runtime::spawn_blocking(|| {
                        fibbo($upper);
                    }));
                }

                for handler in handlers {
                    let _ = ylong_runtime::block_on(handler).unwrap();
                }
            }));
        }
    };
}

#[cfg(test)]
mod tokio_spawn_blocking_bench {
    extern crate test;

    use std::hint::black_box;

    use test::Bencher;

    pub use crate::task_helpers::{fibbo, tokio_runtime};

    tokio_spawn_blocking_task!(tokio_runtime(), tokio_blocking_task_10_15, 10, 15);
    tokio_spawn_blocking_task!(tokio_runtime(), tokio_blocking_task_120_15, 100, 15);
}

#[cfg(test)]
mod ylong_spawn_blocking_bench {
    extern crate test;

    use std::hint::black_box;

    use test::Bencher;

    pub use crate::task_helpers::fibbo;

    ylong_spawn_blocking_task!(ylong_blocking_task_10_15, 10, 15);
    ylong_spawn_blocking_task!(ylong_blocking_task_100_15, 100, 15);
}
