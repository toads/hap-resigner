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

use libc::c_int;

pub(crate) fn socket_new(domain: c_int, socket_type: c_int) -> io::Result<c_int> {
    #[cfg(target_os = "linux")]
    let socket_type = socket_type | libc::SOCK_NONBLOCK | libc::SOCK_CLOEXEC;
    let socket = syscall!(socket(domain, socket_type, 0))?;

    #[cfg(target_os = "macos")]
    set_non_block(socket)?;
    Ok(socket)
}

#[cfg(target_os = "macos")]
pub(crate) fn set_non_block(socket: c_int) -> io::Result<()> {
    if let Err(e) = syscall!(setsockopt(
        socket,
        libc::SOL_SOCKET,
        libc::SO_NOSIGPIPE,
        &1 as *const libc::c_int as *const libc::c_void,
        std::mem::size_of::<libc::c_int>() as libc::socklen_t
    )) {
        let _ = syscall!(close(socket));
        return Err(e);
    }

    if let Err(e) = syscall!(fcntl(socket, libc::F_SETFL, libc::O_NONBLOCK)) {
        let _ = syscall!(close(socket));
        return Err(e);
    }

    if let Err(e) = syscall!(fcntl(socket, libc::F_SETFD, libc::FD_CLOEXEC)) {
        let _ = syscall!(close(socket));
        return Err(e);
    }
    Ok(())
}
