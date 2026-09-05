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

use std::io;

use crate::{Interest, Selector, Token};

/// Fd type on Linux
#[cfg(unix)]
pub type Fd = i32;

/// RawSocket on Windows
#[cfg(target_os = "windows")]
pub type Fd = std::os::windows::io::RawSocket;

/// Source Trait that every connection requires async polling in [`crate::Poll`]
/// needs to implement. [`crate::Poll`] will asynchronously poll out
/// connections, and handle it.
pub trait Source {
    /// Registers the connection into [`crate::Poll`]
    fn register(
        &mut self,
        selector: &Selector,
        token: Token,
        interests: Interest,
    ) -> io::Result<()>;

    /// Deregisters the connection from [`crate::Poll`].
    fn deregister(&mut self, selector: &Selector) -> io::Result<()>;

    /// Returns the raw fd of this IO.
    fn get_fd(&self) -> Fd;
}
