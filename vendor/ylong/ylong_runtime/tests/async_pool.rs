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

#![cfg(all(
    target_os = "linux",
    not(feature = "ffrt"),
    feature = "multi_instance_runtime"
))]

use std::ffi::OsString;
use std::fs;
use std::mem::{size_of, zeroed};

use libc::{
    c_long, cpu_set_t, getpid, sched_getaffinity, sysconf, CPU_ISSET, _SC_NPROCESSORS_ONLN,
};
use ylong_runtime::builder::RuntimeBuilder;

// Simple asynchronous tasks
async fn test_future(num: usize) -> usize {
    num
}

// Complex asynchronous tasks
async fn test_multi_future(i: usize, j: usize) -> usize {
    let result_one = test_future(i).await;
    let result_two = test_future(j).await;

    result_one + result_two
}

// Multi-level nested asynchronous tasks
async fn test_nested_future(i: usize, j: usize) -> usize {
    test_multi_future(i, j).await
}

// Gets the pid of all current threads (including the main thread)
unsafe fn dump_dir() -> Vec<OsString> {
    let current_pid = getpid();
    let dir = format!("/proc/{}/task", current_pid.to_string().as_str());
    let mut result = Vec::new();

    for entry in fs::read_dir(dir.as_str()).expect("read failed") {
        result.push(entry.unwrap().file_name());
    }
    result
}

// Get the name of the thread based on the thread pid
unsafe fn name_of_pid(pid: &str) -> Option<String> {
    let current_pid = getpid();
    let path = format!(
        "/proc/{}/task/{}/status",
        current_pid.to_string().as_str(),
        pid
    );

    match fs::read_to_string(path) {
        Ok(mut result) => {
            let times_one = result.find('\t').unwrap();
            let times_two = result.find('\n').unwrap();

            Some(result.drain(times_one + 1..times_two).collect())
        }
        Err(_) => None,
    }
}

fn get_other_thread_affinity(pid: i32) -> Vec<usize> {
    unsafe {
        let mut vec = vec![];
        let cpus = get_cpu_num() as usize;
        let mut set = new_cpu_set();
        sched_getaffinity(pid, size_of::<cpu_set_t>(), &mut set);
        for i in 0..cpus {
            if CPU_ISSET(i, &set) {
                vec.push(i);
            }
        }
        vec
    }
}

/// Returns an empty cpu set
fn new_cpu_set() -> cpu_set_t {
    unsafe { zeroed::<cpu_set_t>() }
}

fn get_cpu_num() -> c_long {
    unsafe { sysconf(_SC_NPROCESSORS_ONLN) }
}

/// SDV test cases for asynchronous thread pool
///
/// # Brief
/// 1. Constructed environment：
///     1. ASYNCHRONOUS THREAD POOL CAPACITY TOTAL SET TO 1.
///     2. WHETHER TO TIE THE CORE IS_AFFINITY SET TO TRUE.
///     3. THE THREAD NAME IS SET TO "1".
///     4. THE THREAD STACK SIZE IS SET TO 10.
/// 2. Asynchronous tasks：
///     1. Simple asynchronous tasks.
///     2. Complex asynchronous tasks.
///     3. Multi-level nested asynchronous tasks.
#[test]
fn sdv_async_pool_001() {
    let total = 1;
    let is_affinity = true;
    let worker_name = String::from("async_pool_001");
    let stack_size = 10;

    let runtime = RuntimeBuilder::new_multi_thread()
        .worker_name(worker_name)
        .worker_stack_size(stack_size)
        .worker_num(total)
        .is_affinity(is_affinity)
        .build()
        .unwrap();

    let handles = vec![
        runtime.spawn(test_future(1)),
        runtime.spawn(test_multi_future(1, 2)),
        runtime.spawn(test_nested_future(1, 2)),
    ];

    unsafe {
        for dir in dump_dir().iter() {
            let pid = dir.to_str().unwrap().parse::<i32>().unwrap();
            if let Some(name) = name_of_pid(pid.to_string().as_str()) {
                if name == *"async-0-async_pool_001" {
                    #[cfg(target_os = "linux")]
                    assert_eq!(get_other_thread_affinity(pid).len(), 1);
                    break;
                }
            }
        }
    }

    for (times, handle) in handles.into_iter().enumerate() {
        let result = runtime.block_on(handle).unwrap();
        if times == 0 {
            assert_eq!(result, 1);
        }
        if times == 1 {
            assert_eq!(result, 3);
        }
        if times == 2 {
            assert_eq!(result, 3);
        }
    }
}

/// SDV test cases for asynchronous thread pool
///
/// # Brief
/// 1. Constructed environment：
///     1. Asynchronous thread pool capacity total set to 64.
///     2. Whether to tie the core is_affinity set to true.
///     3. The thread name is set to "1".
///     4. The thread stack size is set to 20.
/// 2. Asynchronous tasks：
///     1. Simple asynchronous tasks.
///     2. Complex asynchronous tasks.
///     3. Multi-level nested asynchronous tasks.
#[test]
fn sdv_async_pool_002() {
    let total = 64;
    let is_affinity = true;
    let worker_name = String::from("async_pool_002");
    let stack_size = 20;

    let runtime = RuntimeBuilder::new_multi_thread()
        .worker_name(worker_name)
        .worker_stack_size(stack_size)
        .worker_num(total)
        .is_affinity(is_affinity)
        .build()
        .unwrap();

    let handles = vec![
        runtime.spawn(test_future(1)),
        runtime.spawn(test_multi_future(1, 2)),
        runtime.spawn(test_nested_future(1, 2)),
    ];

    unsafe {
        for dir in dump_dir().iter() {
            let pid = dir.to_str().unwrap().parse::<i32>().unwrap();
            if let Some(name) = name_of_pid(pid.to_string().as_str()) {
                if name == *"async-0-async_pool_002" {
                    #[cfg(target_os = "linux")]
                    assert_eq!(get_other_thread_affinity(pid).len(), 1);
                    break;
                }
            }
        }
    }

    for (times, handle) in handles.into_iter().enumerate() {
        let result = runtime.block_on(handle).unwrap();
        if times == 0 {
            assert_eq!(result, 1);
        }
        if times == 1 {
            assert_eq!(result, 3);
        }
        if times == 2 {
            assert_eq!(result, 3);
        }
    }
}

/// SDV test cases for asynchronous thread pool
///
/// # Brief
/// 1. Constructed environment：
///     1. Asynchronous thread pool capacity total set to 0.
///     2. Whether to tie the core is_affinity set to true.
///     3. The thread name is set to "2".
///     4. The thread stack size is set to 10.
/// 2. Asynchronous tasks：
///     1. Simple asynchronous tasks.
///     2. Complex asynchronous tasks.
///     3. Multi-level nested asynchronous tasks.
#[test]
fn sdv_async_pool_003() {
    let total = 0;
    let is_affinity = true;
    let worker_name = String::from("async_pool_003");
    let stack_size = 10;

    let runtime = RuntimeBuilder::new_multi_thread()
        .worker_name(worker_name)
        .worker_stack_size(stack_size)
        .worker_num(total)
        .is_affinity(is_affinity)
        .build()
        .unwrap();

    let handles = vec![
        runtime.spawn(test_future(1)),
        runtime.spawn(test_multi_future(1, 2)),
        runtime.spawn(test_nested_future(1, 2)),
    ];

    unsafe {
        for dir in dump_dir().iter() {
            let pid = dir.to_str().unwrap().parse::<i32>().unwrap();
            if let Some(name) = name_of_pid(pid.to_string().as_str()) {
                if name == *"async-0-async_pool_003" {
                    #[cfg(target_os = "linux")]
                    assert_eq!(get_other_thread_affinity(pid).len(), 1);
                    break;
                }
            }
        }
    }

    for (times, handle) in handles.into_iter().enumerate() {
        let result = runtime.block_on(handle).unwrap();
        if times == 0 {
            assert_eq!(result, 1);
        }
        if times == 1 {
            assert_eq!(result, 3);
        }
        if times == 2 {
            assert_eq!(result, 3);
        }
    }
}

/// SDV test cases for asynchronous thread pool
///
/// # Brief
/// 1. Constructed environment：
///     1. Asynchronous thread pool capacity total set to 65.
///     2. Whether to tie the core is_affinity set to true.
///     3. The thread name is set to "2".
///     4. The thread stack size is set to 10.
/// 2. Asynchronous tasks：
///     1. Simple asynchronous tasks.
///     2. Complex asynchronous tasks.
///     3. Multi-level nested asynchronous tasks.
#[test]
fn sdv_async_pool_004() {
    let total = 65;
    let is_affinity = true;
    let worker_name = String::from("async_pool_004");
    let stack_size = 20;

    let runtime = RuntimeBuilder::new_multi_thread()
        .worker_name(worker_name)
        .worker_stack_size(stack_size)
        .worker_num(total)
        .is_affinity(is_affinity)
        .build()
        .unwrap();

    let handles = vec![
        runtime.spawn(test_future(1)),
        runtime.spawn(test_multi_future(1, 2)),
        runtime.spawn(test_nested_future(1, 2)),
    ];

    unsafe {
        for dir in dump_dir().iter() {
            let pid = dir.to_str().unwrap().parse::<i32>().unwrap();
            if let Some(name) = name_of_pid(pid.to_string().as_str()) {
                if name == *"async-0-async_pool_004" {
                    #[cfg(target_os = "linux")]
                    assert_eq!(get_other_thread_affinity(pid).len(), 1);
                    break;
                }
            }
        }
    }

    for (times, handle) in handles.into_iter().enumerate() {
        let result = runtime.block_on(handle).unwrap();
        if times == 0 {
            assert_eq!(result, 1);
        }
        if times == 1 {
            assert_eq!(result, 3);
        }
        if times == 2 {
            assert_eq!(result, 3);
        }
    }
}

/// SDV test cases for asynchronous thread pool
///
/// # Brief
/// 1. Constructed environment：
///     1. Asynchronous thread pool capacity total set to 1.
///     2. Whether to tie the core is_affinity set to false.
///     3. The thread name is set to "1".
///     4. The thread stack size is set to 10.
/// 2. Asynchronous tasks：
///     1. Simple asynchronous tasks.
///     2. Complex asynchronous tasks.
///     3. Multi-level nested asynchronous tasks.
#[test]
fn sdv_async_pool_005() {
    let total = 1;
    let is_affinity = false;
    let worker_name = String::from("async_pool_005");
    let stack_size = 10;

    let runtime = RuntimeBuilder::new_multi_thread()
        .worker_name(worker_name)
        .worker_stack_size(stack_size)
        .worker_num(total)
        .is_affinity(is_affinity)
        .build()
        .unwrap();

    let handles = vec![
        runtime.spawn(test_future(1)),
        runtime.spawn(test_multi_future(1, 2)),
        runtime.spawn(test_nested_future(1, 2)),
    ];

    unsafe {
        for dir in dump_dir().iter() {
            let pid = dir.to_str().unwrap().parse::<i32>().unwrap();
            if let Some(name) = name_of_pid(pid.to_string().as_str()) {
                if name == *"async-0-async_pool_005" {
                    #[cfg(target_os = "linux")]
                    assert_eq!(get_other_thread_affinity(pid).len(), get_cpu_num() as usize);
                    break;
                }
            }
        }
    }

    for (times, handle) in handles.into_iter().enumerate() {
        let result = runtime.block_on(handle).unwrap();
        if times == 0 {
            assert_eq!(result, 1);
        }
        if times == 1 {
            assert_eq!(result, 3);
        }
        if times == 2 {
            assert_eq!(result, 3);
        }
    }
}

/// SDV test cases for asynchronous thread pool
///
/// # Brief
/// 1. Constructed environment：
///     1. Asynchronous thread pool capacity total set to 64.
///     2. Whether to tie the core is_affinity set to false.
///     3. The thread name is set to "1".
///     4. The thread stack size is set to 20.
/// 2. Asynchronous tasks：
///     1. Simple asynchronous tasks.
///     2. Complex asynchronous tasks.
///     3. Multi-level nested asynchronous tasks.
#[test]
fn sdv_async_pool_006() {
    let total = 64;
    let is_affinity = false;
    let worker_name = String::from("async_pool_006");
    let stack_size = 20;

    let runtime = RuntimeBuilder::new_multi_thread()
        .worker_name(worker_name)
        .worker_stack_size(stack_size)
        .worker_num(total)
        .is_affinity(is_affinity)
        .build()
        .unwrap();

    let handles = vec![
        runtime.spawn(test_future(1)),
        runtime.spawn(test_multi_future(1, 2)),
        runtime.spawn(test_nested_future(1, 2)),
    ];

    unsafe {
        for dir in dump_dir().iter() {
            let pid = dir.to_str().unwrap().parse::<i32>().unwrap();
            if let Some(name) = name_of_pid(pid.to_string().as_str()) {
                if name == *"async-0-async_pool_006" {
                    #[cfg(target_os = "linux")]
                    assert_eq!(get_other_thread_affinity(pid).len(), get_cpu_num() as usize);
                    break;
                }
            }
        }
    }

    for (times, handle) in handles.into_iter().enumerate() {
        let result = runtime.block_on(handle).unwrap();
        if times == 0 {
            assert_eq!(result, 1);
        }
        if times == 1 {
            assert_eq!(result, 3);
        }
        if times == 2 {
            assert_eq!(result, 3);
        }
    }
}

/// SDV test cases for asynchronous thread pool
///
/// # Brief
/// 1. Constructed environment
///     1. Asynchronous thread pool capacity total set to 0.
///     2. Whether to tie the core is_affinity set to false.
///     3. The thread name is set to "2".
///     4. The thread stack size is set to 10.
/// 2. Asynchronous tasks
///     1. Simple asynchronous tasks.
///     2. Complex asynchronous tasks.
///     3. Multi-level nested asynchronous tasks.
#[test]
fn sdv_async_pool_007() {
    let total = 0;
    let is_affinity = false;
    let worker_name = String::from("async_pool_007");
    let stack_size = 10;

    let runtime = RuntimeBuilder::new_multi_thread()
        .worker_name(worker_name)
        .worker_stack_size(stack_size)
        .worker_num(total)
        .is_affinity(is_affinity)
        .build()
        .unwrap();

    let handles = vec![
        runtime.spawn(test_future(1)),
        runtime.spawn(test_multi_future(1, 2)),
        runtime.spawn(test_nested_future(1, 2)),
    ];

    unsafe {
        for dir in dump_dir().iter() {
            let pid = dir.to_str().unwrap().parse::<i32>().unwrap();
            if let Some(name) = name_of_pid(pid.to_string().as_str()) {
                if name == *"async-0-async_pool_007" {
                    #[cfg(target_os = "linux")]
                    assert_eq!(get_other_thread_affinity(pid).len(), get_cpu_num() as usize);
                    break;
                }
            }
        }
    }

    for (times, handle) in handles.into_iter().enumerate() {
        let result = runtime.block_on(handle).unwrap();
        if times == 0 {
            assert_eq!(result, 1);
        }
        if times == 1 {
            assert_eq!(result, 3);
        }
        if times == 2 {
            assert_eq!(result, 3);
        }
    }
}

/// SDV test cases for asynchronous thread pool
///
/// # Brief
/// 1. Constructed environment：
///     1. Asynchronous thread pool capacity total set to 65.
///     2. Whether to tie the core is_affinity set to false.
///     3. The thread name is set to "2".
///     4. The thread stack size is set to 20.
/// 2. Asynchronous tasks：
///     1. Simple asynchronous tasks.
///     2. Complex asynchronous tasks.
///     3. Multi-level nested asynchronous tasks.
#[test]
fn sdv_async_pool_008() {
    let total = 65;
    let is_affinity = false;
    let worker_name = String::from("async_pool_008");
    let stack_size = 20;

    let runtime = RuntimeBuilder::new_multi_thread()
        .worker_name(worker_name)
        .worker_stack_size(stack_size)
        .worker_num(total)
        .is_affinity(is_affinity)
        .build()
        .unwrap();

    let handles = vec![
        runtime.spawn(test_future(1)),
        runtime.spawn(test_multi_future(1, 2)),
        runtime.spawn(test_nested_future(1, 2)),
    ];

    unsafe {
        for dir in dump_dir().iter() {
            let pid = dir.to_str().unwrap().parse::<i32>().unwrap();
            if let Some(name) = name_of_pid(pid.to_string().as_str()) {
                if name == *"async-0-async_pool_008" {
                    #[cfg(target_os = "linux")]
                    assert_eq!(get_other_thread_affinity(pid).len(), get_cpu_num() as usize);
                    break;
                }
            }
        }
    }

    for (times, handle) in handles.into_iter().enumerate() {
        let result = runtime.block_on(handle).unwrap();
        if times == 0 {
            assert_eq!(result, 1);
        }
        if times == 1 {
            assert_eq!(result, 3);
        }
        if times == 2 {
            assert_eq!(result, 3);
        }
    }
}
