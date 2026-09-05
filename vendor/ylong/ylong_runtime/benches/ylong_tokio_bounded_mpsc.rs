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

//! Benchmarks for bounded mpsc

#![feature(test)]
extern crate core;

mod task_helpers;

extern crate test;

#[macro_export]
macro_rules! tokio_bounded_mpsc {
    ($runtime: expr, $bench: ident, $num: literal, $loop_num: literal) => {
        #[bench]
        fn $bench(b: &mut Bencher) {
            let runtime = $runtime;
            b.iter(black_box(|| {
                let (sender, mut receiver) = channel(10);
                let mut handlers = vec![];
                let handle = runtime.spawn(async move {
                    for _ in 0..$num * $loop_num {
                        let res = receiver.recv().await.unwrap();
                        assert_eq!(res, 1);
                    }
                });
                handlers.push(handle);

                for _ in 0..$num {
                    let producer = sender.clone();
                    let handle = runtime.spawn(async move {
                        for _ in 0..$loop_num {
                            producer.send(1).await.unwrap();
                        }
                    });
                    handlers.push(handle);
                }

                for handle in handlers {
                    let _ = runtime.block_on(handle).unwrap();
                }
            }));
        }
    };
}

#[macro_export]
macro_rules! ylong_bounded_mpsc {
    ($bench: ident, $num: literal, $loop_num: literal) => {
        #[bench]
        fn $bench(b: &mut Bencher) {
            b.iter(black_box(|| {
                let (sender, mut receiver) = bounded_channel(10);
                let mut handlers = vec![];
                let handle = ylong_runtime::spawn(async move {
                    for _ in 0..$num * $loop_num {
                        let res = receiver.recv().await.unwrap();
                        assert_eq!(res, 1);
                    }
                });
                handlers.push(handle);

                for _ in 0..$num {
                    let producer = sender.clone();
                    let handle = ylong_runtime::spawn(async move {
                        for _ in 0..$loop_num {
                            producer.send(1).await.unwrap();
                        }
                    });
                    handlers.push(handle);
                }

                for handle in handlers {
                    let _ = ylong_runtime::block_on(handle).unwrap();
                }
            }));
        }
    };
}

#[cfg(test)]
mod tokio_bounded_mpsc_bench {
    extern crate test;

    use std::hint::black_box;

    use test::Bencher;
    use tokio::sync::mpsc::channel;

    pub use crate::task_helpers::tokio_runtime;
    tokio_bounded_mpsc!(tokio_runtime(), tokio_spawn_blocking_1_1000, 1, 1000);
    tokio_bounded_mpsc!(tokio_runtime(), tokio_spawn_blocking_5_1000, 5, 1000);
    tokio_bounded_mpsc!(tokio_runtime(), tokio_spawn_blocking_10_1000, 10, 1000);
    tokio_bounded_mpsc!(tokio_runtime(), tokio_spawn_blocking_50_1000, 50, 1000);
}

#[cfg(test)]
mod ylong_bounded_mpsc_bench {
    extern crate test;

    use std::hint::black_box;

    use test::Bencher;
    use ylong_runtime::sync::mpsc::bounded_channel;

    ylong_bounded_mpsc!(ylong_spawn_blocking_1_1000, 1, 1000);
    ylong_bounded_mpsc!(ylong_spawn_blocking_5_1000, 5, 1000);
    ylong_bounded_mpsc!(ylong_spawn_blocking_10_1000, 10, 1000);
    ylong_bounded_mpsc!(ylong_spawn_blocking_50_1000, 50, 1000);
}
