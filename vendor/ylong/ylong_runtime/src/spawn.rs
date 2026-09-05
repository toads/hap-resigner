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

use std::future::Future;

use crate::executor::global_default_async;
use crate::task::join_handle::JoinHandle;
use crate::task::TaskBuilder;

cfg_not_ffrt! {
    use crate::executor::global_default_blocking;
}

cfg_ffrt! {
    use std::pin::Pin;
    use std::task::{Context, Poll};
    use crate::ffrt::spawner::spawn;

    struct BlockingTask<T>(Option<T>);

    impl<T> Unpin for BlockingTask<T> {}

    impl<T, R> Future for BlockingTask<T>
        where
            T: FnOnce() -> R,
    {
        type Output = R;

        fn poll(mut self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Self::Output> {
            // Task won't be polled again after finished
            let func = self.0.take().expect("blocking tasks cannot be polled after finished");
            Poll::Ready(func())
        }
    }

    /// Spawns a task on the blocking pool.
    pub(crate) fn spawn_blocking<T, R>(builder: &TaskBuilder, task: T) -> JoinHandle<R>
        where
            T: FnOnce() -> R,
            T: Send + 'static,
            R: Send + 'static,
    {
        let task = BlockingTask(Some(task));
        spawn(task, builder)
    }
}

#[cfg(not(feature = "ffrt"))]
/// Spawns a task on the blocking pool.
pub(crate) fn spawn_blocking<T, R>(builder: &TaskBuilder, task: T) -> JoinHandle<R>
where
    T: FnOnce() -> R,
    T: Send + 'static,
    R: Send + 'static,
{
    let rt = global_default_blocking();
    rt.spawn_blocking(builder, task)
}

/// Gets global default executor, spawns async tasks by the task builder, and
/// returns.
#[inline]
pub(crate) fn spawn_async<T, R>(builder: &TaskBuilder, task: T) -> JoinHandle<R>
where
    T: Future<Output = R>,
    T: Send + 'static,
    R: Send + 'static,
{
    let rt = global_default_async();
    rt.spawn_with_attr(task, builder)
}
