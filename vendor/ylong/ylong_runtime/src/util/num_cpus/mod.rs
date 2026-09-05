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

//! Gets the number of cpus of the machine.
//!
//! Currently this crate supports two platform: `linux` and `windows`

use std::os::raw::c_long;

#[cfg(target_family = "unix")]
pub mod unix;

#[cfg(target_os = "windows")]
pub mod windows;

#[cfg(target_family = "unix")]
use crate::util::num_cpus::unix::get_cpu_num_online;
#[cfg(target_os = "windows")]
use crate::util::num_cpus::windows::get_cpu_num_online;

/// The get_cpu_num function is the external interface, which will automatically
/// call the underlying functions for different operating systems. Linux, using
/// sysconf() function, which gets the number of cpu cores in the available
/// state by default. Windows, using GetSystemInfo() function, which gets the
/// number of cpu cores in the available state by default. # Example
///
/// ```no run
/// use ylong_runtime::util::num_cpus;
///
/// let cpus = num_cpus::get_cpu_num();
/// ```
pub fn get_cpu_num() -> c_long {
    get_cpu_num_online()
}

#[cfg(test)]
mod test {
    use super::*;

    /// UT test cases for num_cpus.
    ///
    /// # Brief
    /// 1. call get_cpu_num and check it greater than zero
    #[test]
    fn ut_num_cpus_test() {
        let cpus = get_cpu_num();
        assert!(cpus > 0);
    }
}
