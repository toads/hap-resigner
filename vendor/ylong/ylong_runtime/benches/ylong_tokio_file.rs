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

//! Benchmarks for large file io operations. Covers more scenarios than
//! "ylong_tokio_async_file.rs" does

#![feature(test)]
pub const KB: usize = 1024;
pub const TASK_NUM: usize = 10;
pub const THREAD_NUM: usize = 16;

mod task_helpers;

#[macro_export]
macro_rules! async_write {
    ($content: expr, $file_size: expr) => {
        let dir = get_file_dir($file_size);
        for _ in 0..TASK_NUM {
            let mut file = File::create(dir.clone()).await.unwrap();
            let _ = file.write_all($content).await.unwrap();
        }
    };
}

#[macro_export]
macro_rules! async_read {
    ($file_size: expr) => {
        let dir = get_file_dir($file_size);
        for _ in 0..TASK_NUM {
            let mut file = File::open(dir.clone()).await.unwrap();
            let mut buffer = Vec::new();
            file.read_to_end(&mut buffer).await.unwrap();
            assert!(buffer.len() == $file_size * KB);
        }
    };
}

#[macro_export]
macro_rules! tokio_file_io_write {
    ($runtime: expr, $bench: ident, $file_size: literal) => {
        #[bench]
        fn $bench(b: &mut Bencher) {
            use tokio::fs::File;
            use tokio::io::AsyncWriteExt;

            use crate::KB;
            let runtime = $runtime;
            let file_size = $file_size;
            let content = get_writer_buffer(file_size);
            assert!(content.len() == file_size * KB);

            b.iter(black_box(|| {
                let task = || async {
                    async_write!(&content, file_size);
                };
                runtime.block_on(task());
            }))
        }
    };
}

#[macro_export]
macro_rules! tokio_file_io_read {
    ($runtime: expr, $bench: ident, $file_size: literal) => {
        #[bench]
        fn $bench(b: &mut Bencher) {
            use tokio::fs::File;
            use tokio::io::{AsyncReadExt, AsyncWriteExt};

            use crate::KB;
            let runtime = $runtime;
            let file_size = $file_size;
            let content = get_writer_buffer(file_size);
            assert!(content.len() == file_size * KB);
            runtime.block_on(async move {
                async_write!(&content, file_size);
            });
            b.iter(black_box(|| {
                let task = || async {
                    async_read!(file_size);
                };
                runtime.block_on(task());
            }))
        }
    };
}

#[macro_export]
macro_rules! ylong_file_io_write {
    ($bench: ident, $file_size: literal) => {
        #[bench]
        fn $bench(b: &mut Bencher) {
            use ylong_runtime::fs::File;
            use ylong_runtime::io::AsyncWriteExt;

            use crate::KB;

            let _ = ylong_runtime::builder::RuntimeBuilder::new_multi_thread()
                .max_blocking_pool_size(THREAD_NUM as u8)
                .build_global();

            let file_size = $file_size;
            let content = get_writer_buffer(file_size);
            assert!(content.len() == file_size * KB);

            b.iter(black_box(|| {
                let task = || async {
                    async_write!(&content, file_size);
                };
                ylong_runtime::block_on(task());
            }));
        }
    };
}

#[macro_export]
macro_rules! ylong_file_io_read {
    ($bench: ident, $file_size: literal) => {
        #[bench]
        fn $bench(b: &mut Bencher) {
            use ylong_runtime::fs::File;
            use ylong_runtime::io::{AsyncReadExt, AsyncWriteExt};

            use crate::KB;

            let _ = ylong_runtime::builder::RuntimeBuilder::new_multi_thread()
                .max_blocking_pool_size(THREAD_NUM as u8)
                .build_global();

            let file_size = $file_size;
            let content = get_writer_buffer(file_size);
            assert!(content.len() == file_size * KB);
            ylong_runtime::block_on(async move {
                async_write!(&content, file_size);
            });

            b.iter(black_box(|| {
                let task = || async {
                    async_read!(file_size);
                };
                ylong_runtime::block_on(task());
            }))
        }
    };
}

pub fn tokio_runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_multi_thread()
        .max_blocking_threads(THREAD_NUM)
        .enable_all()
        .build()
        .unwrap()
}

#[cfg(test)]
mod file_write_bench {
    extern crate test;

    use std::hint::black_box;

    use test::Bencher;

    pub use crate::task_helpers::{get_file_dir, get_writer_buffer};
    use crate::{tokio_runtime, TASK_NUM, THREAD_NUM};

    tokio_file_io_write!(tokio_runtime(), tokio_file_write_1kb, 1);

    tokio_file_io_write!(tokio_runtime(), tokio_file_write_16kb, 16);

    tokio_file_io_write!(tokio_runtime(), tokio_file_write_256kb, 256);

    tokio_file_io_write!(tokio_runtime(), tokio_file_write_1mb, 1024);

    tokio_file_io_write!(tokio_runtime(), tokio_file_write_8mb, 8192);

    tokio_file_io_write!(tokio_runtime(), tokio_file_write_64mb, 65536);

    ylong_file_io_write!(ylong_file_write_1kb, 1);

    ylong_file_io_write!(ylong_file_write_16kb, 16);

    ylong_file_io_write!(ylong_file_write_256kb, 256);

    ylong_file_io_write!(ylong_file_write_1mb, 1024);

    ylong_file_io_write!(ylong_file_write_8mb, 8192);

    ylong_file_io_write!(ylong_file_write_64mb, 65536);
}

#[cfg(test)]
mod file_read_bench {
    extern crate test;

    use std::hint::black_box;

    use test::Bencher;

    pub use crate::task_helpers::{get_file_dir, get_writer_buffer};
    use crate::{tokio_runtime, TASK_NUM, THREAD_NUM};

    tokio_file_io_read!(tokio_runtime(), tokio_file_read_1kb, 1);

    tokio_file_io_read!(tokio_runtime(), tokio_file_read_16kb, 16);

    tokio_file_io_read!(tokio_runtime(), tokio_file_read_256kb, 256);

    tokio_file_io_read!(tokio_runtime(), tokio_file_read_1mb, 1024);

    tokio_file_io_read!(tokio_runtime(), tokio_file_read_8mb, 8192);

    tokio_file_io_read!(tokio_runtime(), tokio_file_read_64mb, 65536);

    ylong_file_io_read!(ylong_file_read_1kb, 1);

    ylong_file_io_read!(ylong_file_read_16kb, 16);

    ylong_file_io_read!(ylong_file_read_256kb, 256);

    ylong_file_io_read!(ylong_file_read_1mb, 1024);

    ylong_file_io_read!(ylong_file_read_8mb, 8192);

    ylong_file_io_read!(ylong_file_read_64mb, 65536);
}
