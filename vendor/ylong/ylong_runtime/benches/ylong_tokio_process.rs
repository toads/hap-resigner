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

//! Benchmarks for the process.

#![feature(test)]
#![cfg(all(unix, feature = "process"))]

extern crate core;

mod task_helpers;

macro_rules! tokio_process_task {
    ($runtime: expr, $bench: ident, $task_num: literal) => {
        #[bench]
        fn $bench(b: &mut Bencher) {
            let runtime = $runtime;
            b.iter(black_box(|| {
                let mut handlers = Vec::new();
                for _ in 0..$task_num {
                    handlers.push(runtime.spawn(async {
                        let mut command = tokioCommand::new("echo");
                        command.arg("Hello, world!");
                        let output = command.output().await.unwrap();

                        assert!(output.status.success());
                        assert_eq!(output.stdout.as_slice(), b"Hello, world!\n");
                        assert!(output.stderr.is_empty());
                    }));
                }
                for handler in handlers {
                    runtime.block_on(handler).unwrap();
                }
            }));
        }
    };
}

macro_rules! ylong_process_task {
    ($bench: ident, $task_num: literal) => {
        #[bench]
        fn $bench(b: &mut Bencher) {
            b.iter(black_box(|| {
                let mut handlers = Vec::new();
                for _ in 0..$task_num {
                    handlers.push(ylong_runtime::spawn(async {
                        let mut command = Command::new("echo");
                        command.arg("Hello, world!");
                        let output = command.output().await.unwrap();

                        assert!(output.status.success());
                        assert_eq!(output.stdout.as_slice(), b"Hello, world!\n");
                        assert!(output.stderr.is_empty());
                    }));
                }
                for handler in handlers {
                    ylong_runtime::block_on(handler).unwrap();
                }
            }));
        }
    };
}

#[cfg(test)]
mod process_bench {
    extern crate test;

    use std::hint::black_box;

    use test::Bencher;
    use tokio::process::Command as tokioCommand;
    use ylong_runtime::process::Command;

    pub use crate::task_helpers::tokio_runtime;

    ylong_process_task!(ylong_process_10, 10);
    tokio_process_task!(tokio_runtime(), tokio_process_10, 10);
    ylong_process_task!(ylong_process_50, 50);
    tokio_process_task!(tokio_runtime(), tokio_process_50, 50);
    ylong_process_task!(ylong_process_100, 100);
    tokio_process_task!(tokio_runtime(), tokio_process_100, 100);
    ylong_process_task!(ylong_process_200, 200);
    tokio_process_task!(tokio_runtime(), tokio_process_200, 200);
}
