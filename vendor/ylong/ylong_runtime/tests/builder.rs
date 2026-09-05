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

#[cfg(feature = "multi_instance_runtime")]
mod multi_test {
    use std::sync::{Arc, Mutex};

    use ylong_runtime::builder::RuntimeBuilder;

    // async task
    async fn test_future(num: usize) -> usize {
        num
    }

    /// SDV test cases for `after_start()`.
    ///
    /// # Brief
    /// 1. Create Runtime.
    /// 2. Sender calls after_start() to set variable x to 1.
    /// 3. Executing an async task.
    /// 4. Check if the test results are correct.
    #[test]
    fn sdv_set_builder_after_start() {
        let x = Arc::new(Mutex::new(0));
        let xc = x.clone();

        let runtime = RuntimeBuilder::new_multi_thread()
            .max_blocking_pool_size(4)
            .worker_num(8)
            .after_start(move || {
                let mut a = xc.lock().unwrap();
                *a = 1;
            })
            .build()
            .unwrap();

        let handle = runtime.spawn(test_future(1));
        let _result = runtime.block_on(handle).unwrap();

        let a = x.lock().unwrap();
        assert_eq!(*a, 1);
    }

    /// SDV test cases for `before_stop()`.
    ///
    /// # Brief
    /// 1. Create Runtime.
    /// 2. Sender calls after_start() to set variable x to 1.
    /// 3. Executing an async task.
    /// 4. Check if the test results are correct.
    #[test]
    fn sdv_set_builder_before_stop() {
        let x = Arc::new(Mutex::new(0));
        let xc = x.clone();

        let runtime = RuntimeBuilder::new_multi_thread()
            .max_blocking_pool_size(4)
            .worker_num(8)
            .before_stop(move || {
                let mut a = xc.lock().unwrap();
                *a = 1;
            })
            .build()
            .unwrap();
        let handle = runtime.spawn(test_future(1));
        let _result = runtime.block_on(handle).unwrap();

        drop(runtime);
        let a = x.lock().unwrap();
        assert_eq!(*a, 1);
    }
}

#[cfg(feature = "current_thread_runtime")]
mod current_test {
    use ylong_runtime::builder::RuntimeBuilder;

    /// SDV test cases for `new_current_thread()`.
    ///
    /// # Brief
    /// 1. Create Runtime.
    /// 2. Spawn a new task and block_on it.
    /// 3. Check result is correct.
    /// 4. Block_on a task and check result.
    #[test]
    fn sdv_set_builder_after_start() {
        let runtime = RuntimeBuilder::new_current_thread().build().unwrap();

        let handle = runtime.spawn(async { 1 });
        let result = runtime.block_on(handle).unwrap();
        assert_eq!(result, 1);

        let result = runtime.block_on(async { 1 });
        assert_eq!(result, 1);
    }
}

#[cfg(feature = "ffrt")]
mod ffrt_test {
    use ylong_ffrt::Qos;
    use ylong_runtime::builder::RuntimeBuilder;

    /// SDV test cases for `build_global`.
    ///
    /// # Brief
    /// 1. Configures the RuntimeBuilder
    /// 2. Calls build_global on the builder and checks if the return value is
    ///    ok
    /// 3. Configures another RuntimeBuilder
    /// 4. Calls build_global on the builder and checks if the return value is
    ///    err
    #[test]
    fn sdv_build_global() {
        let ret = RuntimeBuilder::new_multi_thread()
            .max_worker_num_by_qos(Qos::Default, 8)
            .max_worker_num_by_qos(Qos::Background, 0)
            .max_worker_num_by_qos(Qos::UserInteractive, 21)
            .build_global();
        assert!(ret.is_ok());

        let ret = RuntimeBuilder::new_multi_thread()
            .max_worker_num_by_qos(Qos::Default, 8)
            .max_worker_num_by_qos(Qos::Background, 0)
            .max_worker_num_by_qos(Qos::UserInteractive, 21)
            .build_global();
        assert!(ret.is_err());
    }
}
