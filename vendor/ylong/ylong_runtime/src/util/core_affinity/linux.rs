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

//! Wraps Linux core-affinity syscalls.

use std::io::{Error, Result};
use std::mem::{size_of, zeroed};

use libc::{cpu_set_t, sched_setaffinity, CPU_SET};

/// Sets the tied core cpu of the current thread.
///
/// sched_setaffinity function under linux
/// # Example
///
/// ```no run
/// use ylong_runtime::util::core_affinity;
///
/// let ret = core_affinity::set_current_affinity(0).is_ok();
/// ```
pub fn set_current_affinity(cpu: usize) -> Result<()> {
    let res: i32 = unsafe {
        let mut set = new_cpu_set();
        CPU_SET(cpu, &mut set);
        sched_setaffinity(0, size_of::<cpu_set_t>(), &set)
    };
    match res {
        0 => Ok(()),
        _ => Err(Error::last_os_error()),
    }
}

/// Returns an empty cpu set
fn new_cpu_set() -> cpu_set_t {
    unsafe { zeroed::<cpu_set_t>() }
}
