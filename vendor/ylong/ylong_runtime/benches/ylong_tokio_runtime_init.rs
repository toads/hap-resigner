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

use crate::task_helpers::tokio_runtime;

extern crate test;

use std::hint::black_box;

use test::Bencher;

#[bench]
fn tokio_init(b: &mut Bencher) {
    b.iter(black_box(|| {
        let _ = tokio_runtime();
    }));
}

#[cfg(feature = "full")]
#[bench]
fn ylong_init(b: &mut Bencher) {
    b.iter(black_box(|| {
        let _ = ylong_runtime::builder::RuntimeBuilder::new_multi_thread()
            .build()
            .unwrap();
    }));
}
