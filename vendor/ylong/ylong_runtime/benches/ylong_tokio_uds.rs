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

//! Benchmarks for the Uds.

#![feature(test)]
#![cfg(unix)]

extern crate core;

mod task_helpers;

#[macro_export]
macro_rules! tokio_uds_task {
    ($runtime: expr, $bench: ident, $server: ident, $client: ident, $path: literal, $task_num: literal, $loop_num: literal, $buf_size: literal) => {
        pub async fn $server(addr: String) {
            let uds = tokioUnixListener::bind(addr).unwrap();
            let (mut stream, _) = uds.accept().await.unwrap();
            for _ in 0..$loop_num {
                let mut buf = [0; $buf_size];
                stream.read_exact(&mut buf).await.unwrap();
                assert_eq!(buf, [3; $buf_size]);

                let buf = [2; $buf_size];
                stream.write_all(&buf).await.unwrap();
            }
        }

        pub async fn $client(addr: String) {
            let mut uds = tokioUnixStream::connect(addr.clone()).await;
            while uds.is_err() {
                uds = tokioUnixStream::connect(addr.clone()).await;
            }
            let mut uds = uds.unwrap();
            for _ in 0..$loop_num {
                let buf = [3; $buf_size];
                uds.write_all(&buf).await.unwrap();

                let mut buf = [0; $buf_size];
                uds.read_exact(&mut buf).await.unwrap();
                assert_eq!(buf, [2; $buf_size]);
            }
        }

        #[bench]
        fn $bench(b: &mut Bencher) {
            let runtime = $runtime;
            b.iter(black_box(|| {
                let mut handlers = Vec::new();
                for i in 0..$task_num {
                    let addr = $path.to_owned() + &i.to_string();
                    handlers.push(runtime.spawn($server(addr.clone())));
                    handlers.push(runtime.spawn($client(addr.clone())));
                }
                for handler in handlers {
                    runtime.block_on(handler).unwrap();
                }
                for i in 0..$task_num {
                    let addr = $path.to_owned() + &i.to_string();
                    std::fs::remove_file(addr).unwrap();
                }
            }));
        }
    };
}

#[macro_export]
macro_rules! ylong_uds_task {
    ($bench: ident, $server: ident, $client: ident, $path: literal, $task_num: literal, $loop_num: literal, $buf_size: literal) => {
        pub async fn $server(addr: String) {
            let uds = UnixListener::bind(addr).unwrap();
            let (mut stream, _) = uds.accept().await.unwrap();
            for _ in 0..$loop_num {
                let mut buf = [0; $buf_size];
                stream.read_exact(&mut buf).await.unwrap();
                assert_eq!(buf, [3; $buf_size]);

                let buf = [2; $buf_size];
                stream.write_all(&buf).await.unwrap();
            }
        }

        pub async fn $client(addr: String) {
            let mut uds = UnixStream::connect(addr.clone()).await;
            while uds.is_err() {
                uds = UnixStream::connect(addr.clone()).await;
            }
            let mut uds = uds.unwrap();
            for _ in 0..$loop_num {
                let buf = [3; $buf_size];
                uds.write_all(&buf).await.unwrap();

                let mut buf = [0; $buf_size];
                uds.read_exact(&mut buf).await.unwrap();
                assert_eq!(buf, [2; $buf_size]);
            }
        }

        #[bench]
        fn $bench(b: &mut Bencher) {
            ylong_runtime_init();
            b.iter(black_box(|| {
                let mut handlers = Vec::new();
                for i in 0..$task_num {
                    let addr = $path.to_owned() + &i.to_string();
                    handlers.push(ylong_runtime::spawn($server(addr.clone())));
                    handlers.push(ylong_runtime::spawn($client(addr.clone())));
                }
                for handler in handlers {
                    ylong_runtime::block_on(handler).unwrap();
                }
                for i in 0..$task_num {
                    let addr = $path.to_owned() + &i.to_string();
                    std::fs::remove_file(addr).unwrap();
                }
            }));
        }
    };
}

#[cfg(test)]
mod uds_bench {
    extern crate test;

    use std::hint::black_box;

    use test::Bencher;
    use tokio::io::{AsyncReadExt as tokioAsyncReadExt, AsyncWriteExt as tokioAsyncWriteExt};
    use tokio::net::{UnixListener as tokioUnixListener, UnixStream as tokioUnixStream};
    use ylong_runtime::io::{AsyncReadExt, AsyncWriteExt};
    use ylong_runtime::net::{UnixListener, UnixStream};

    pub use crate::task_helpers::{tokio_runtime, ylong_runtime_init};

    ylong_uds_task!(
        ylong_uds_10_1000_100,
        ylong_server1,
        ylong_client1,
        "/tmp/uds_ylong_path",
        10,
        1000,
        100
    );
    tokio_uds_task!(
        tokio_runtime(),
        tokio_uds_10_1000_100,
        tokio_server1,
        tokio_client1,
        "/tmp/uds_tokio_path",
        10,
        1000,
        100
    );
    ylong_uds_task!(
        ylong_uds_10_1000_20000,
        ylong_server2,
        ylong_client2,
        "/tmp/uds_ylong_path",
        10,
        1000,
        20000
    );
    tokio_uds_task!(
        tokio_runtime(),
        tokio_uds_10_1000_20000,
        tokio_server2,
        tokio_client2,
        "/tmp/uds_tokio_path",
        10,
        1000,
        20000
    );
    ylong_uds_task!(
        ylong_uds_10_20_20000,
        ylong_server3,
        ylong_client3,
        "/tmp/uds_ylong_path",
        10,
        20,
        20000
    );
    tokio_uds_task!(
        tokio_runtime(),
        tokio_uds_10_20_20000,
        tokio_server3,
        tokio_client3,
        "/tmp/uds_tokio_path",
        10,
        20,
        20000
    );
    ylong_uds_task!(
        ylong_uds_10_20_100,
        ylong_server4,
        ylong_client4,
        "/tmp/uds_ylong_path",
        10,
        20,
        100
    );
    tokio_uds_task!(
        tokio_runtime(),
        tokio_uds_10_20_100,
        tokio_server4,
        tokio_client4,
        "/tmp/uds_tokio_path",
        10,
        20,
        100
    );
}
