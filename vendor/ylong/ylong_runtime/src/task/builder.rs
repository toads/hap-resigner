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

//! Builder to configure the task. Tasks that get spawned through
//! [`TaskBuilder`] inherit all attributes of this builder.
//!
//! A task has following attributes:
//! - qos
//! - task name

use std::future::Future;

use crate::spawn::{spawn_async, spawn_blocking};
use crate::task::{JoinHandle, Qos};

/// Tasks attribute
#[derive(Clone)]
pub struct TaskBuilder {
    pub(crate) name: Option<String>,
    pub(crate) qos: Option<Qos>,
}

impl Default for TaskBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl TaskBuilder {
    /// Creates a new TaskBuilder with a default setting.
    pub fn new() -> Self {
        TaskBuilder {
            name: None,
            qos: None,
        }
    }

    /// Sets the name of the task.
    pub fn name(mut self, name: String) -> Self {
        self.name = Some(name);
        self
    }

    /// Sets the qos of the task
    pub fn qos(mut self, qos: Qos) -> Self {
        self.qos = Some(qos);
        self
    }

    /// todo: for multiple-instance runtime, should provide a spawn_on
    /// Using the current task setting, spawns a task onto the global runtime.
    pub fn spawn<T, R>(&self, task: T) -> JoinHandle<R>
    where
        T: Future<Output = R>,
        T: Send + 'static,
        R: Send + 'static,
    {
        spawn_async(self, task)
    }

    /// Using the current task setting, spawns a task onto the blocking pool.
    pub fn spawn_blocking<T, R>(&self, task: T) -> JoinHandle<R>
    where
        T: FnOnce() -> R,
        T: Send + 'static,
        R: Send + 'static,
    {
        spawn_blocking(self, task)
    }
}

#[cfg(test)]
mod test {
    use crate::task::{Qos, TaskBuilder};

    #[test]
    fn ut_task() {
        ut_builder_new();
        ut_builder_name();
        ut_builder_pri();
    }

    /// UT test cases for Builder::new
    ///
    /// # Brief
    /// 1. Checks if the object name property is None
    /// 2. Checks if the object pri property is None
    /// 3. Checks if the object worker_id property is None
    /// 4. Checks if the object is_stat property is false
    /// 5. Checks if the object is_insert_front property is false
    fn ut_builder_new() {
        let builder1 = TaskBuilder::new();
        let builder2 = builder1.clone();
        assert_eq!(builder1.name, None);
        assert!(builder2.qos.is_none());
    }

    /// UT test cases for Builder::name
    ///
    /// # Brief
    /// 1. Checks if the object name property is a modified value
    fn ut_builder_name() {
        let builder = TaskBuilder::new();

        let name = String::from("builder_name");
        assert_eq!(builder.name(name.clone()).name.unwrap(), name);
    }

    /// UT test cases for Builder::name
    ///
    /// # Brief
    /// 1. pri set to Background, check return value
    /// 2. pri set to Utility, check return value
    /// 3. pri set to UserInteractive, check return value
    /// 4. pri set to UserInitiated, check return value
    /// 5. pri set to Default, check return value
    fn ut_builder_pri() {
        let builder = TaskBuilder::new();
        let pri = Qos::Background;
        assert_eq!(builder.qos(pri).qos.unwrap(), pri);

        let builder = TaskBuilder::new();
        let pri = Qos::Utility;
        assert_eq!(builder.qos(pri).qos.unwrap(), pri);

        let builder = TaskBuilder::new();
        let pri = Qos::UserInteractive;
        assert_eq!(builder.qos(pri).qos.unwrap(), pri);

        let builder = TaskBuilder::new();
        let pri = Qos::UserInitiated;
        assert_eq!(builder.qos(pri).qos.unwrap(), pri);

        let builder = TaskBuilder::new();
        let pri = Qos::Default;
        assert_eq!(builder.qos(pri).qos.unwrap(), pri);
    }
}
