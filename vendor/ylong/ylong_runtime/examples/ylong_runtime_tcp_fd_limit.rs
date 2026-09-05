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

//! A test for FD overflow

use ylong_runtime::net::TcpListener;

fn main() {
    let handle = ylong_runtime::spawn(async move {
        let mut vec = vec![];
        loop {
            let tcp = TcpListener::bind("127.0.0.1:0").await;
            match tcp {
                Err(e) => {
                    println!("err: {}", e.kind());
                    return;
                }
                Ok(listener) => vec.push(listener),
            }
        }
    });
    ylong_runtime::block_on(handle).unwrap();
}
