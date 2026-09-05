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
use std::panic;
use std::ptr::NonNull;
use std::task::{Context, Poll, Waker};

use crate::error::{ErrorKind, ScheduleError};
use crate::executor::Schedule;
use crate::task::raw::{Header, Inner, TaskMngInfo};
use crate::task::state;
use crate::task::state::StateAction;
use crate::task::waker::WakerRefHeader;

cfg_not_ffrt! {
    use crate::task::Task;
}

pub(crate) struct TaskHandle<T: Future, S: Schedule> {
    task: NonNull<TaskMngInfo<T, S>>,
}

impl<T, S> TaskHandle<T, S>
where
    T: Future,
    S: Schedule,
{
    pub(crate) unsafe fn from_raw(ptr: NonNull<Header>) -> Self {
        TaskHandle {
            task: ptr.cast::<TaskMngInfo<T, S>>(),
        }
    }

    fn header(&self) -> &Header {
        unsafe { self.task.as_ref().header() }
    }

    fn inner(&self) -> &Inner<T, S> {
        unsafe { self.task.as_ref().inner() }
    }
}

impl<T, S> TaskHandle<T, S>
where
    T: Future,
    S: Schedule,
{
    fn finish(self, state: usize, output: Result<T::Output, ScheduleError>) {
        // send result if the JoinHandle is not dropped
        if state::is_care_join_handle(state) {
            self.inner().send_result(output);
        } else {
            self.inner().turning_to_used_data();
        }

        let cur = match self.header().state.turning_to_finish() {
            Ok(cur) => cur,
            Err(e) => panic!("{}", e.as_str()),
        };

        if state::is_set_waker(cur) {
            self.inner().wake_join();
        }
        self.drop_ref();
    }

    pub(crate) fn release(self) {
        unsafe { drop(Box::from_raw(self.task.as_ptr())) };
    }

    pub(crate) fn drop_ref(self) {
        let prev = self.header().state.dec_ref();
        if state::is_last_ref_count(prev) {
            self.release();
        }
    }

    pub(crate) fn get_result(self, out: &mut Poll<std::result::Result<T::Output, ScheduleError>>) {
        *out = Poll::Ready(self.inner().turning_to_get_data());
    }

    pub(crate) fn drop_join_handle(self) {
        if self.header().state.try_turning_to_un_join_handle() {
            return;
        }

        match self.header().state.turn_to_un_join_handle() {
            Ok(_) => {}
            Err(_) => {
                self.inner().turning_to_used_data();
            }
        }
        self.drop_ref();
    }

    fn set_waker_inner(&self, des_waker: Waker, cur_state: usize) -> Result<usize, usize> {
        assert!(
            state::is_care_join_handle(cur_state),
            "set waker failed: the join handle has been dropped"
        );
        assert!(
            !state::is_set_waker(cur_state),
            "set waker failed: the task already has a waker set"
        );

        unsafe {
            let waker = self.inner().waker.get();
            *waker = Some(des_waker);
        }
        let result = self.header().state.turn_to_set_waker();
        if result.is_err() {
            unsafe {
                let waker = self.inner().waker.get();
                *waker = None;
            }
        }
        result
    }

    pub(crate) fn set_waker(self, cur: usize, des_waker: &Waker) -> bool {
        let res = if state::is_set_waker(cur) {
            let is_same_waker = unsafe {
                // the status is set_waker, so waker must be set already
                let waker = self.inner().waker.get();
                (*waker)
                    .as_ref()
                    .expect("task status is set_waker, but waker is missing")
                    .will_wake(des_waker)
            };
            // we don't register the same waker
            if is_same_waker {
                return false;
            }
            self.header()
                .state
                .turn_to_un_set_waker()
                .and_then(|cur| self.set_waker_inner(des_waker.clone(), cur))
        } else {
            self.set_waker_inner(des_waker.clone(), cur)
        };

        if let Err(cur) = res {
            assert!(
                state::is_finished(cur),
                "setting waker should only be failed dur to task completion"
            );
            return true;
        }

        false
    }
}

#[cfg(not(feature = "ffrt"))]
impl<T, S> TaskHandle<T, S>
where
    T: Future,
    S: Schedule,
{
    // Runs the task
    pub(crate) fn run(self) {
        let action = self.header().state.turning_to_running();

        match action {
            StateAction::Success => {}
            StateAction::Canceled(cur) => {
                let output = self.get_canceled();
                return self.finish(cur, Err(output));
            }
            StateAction::Failed(state) => panic!("task state invalid: {state}"),
            _ => unreachable!(),
        };

        // turn the task header into a waker
        let waker = WakerRefHeader::<'_>::new::<T>(self.header());
        let mut context = Context::from_waker(&waker);

        let res = panic::catch_unwind(panic::AssertUnwindSafe(|| {
            self.inner().poll(&mut context).map(Ok)
        }));

        let cur = self.header().state.get_current_state();
        match res {
            Ok(Poll::Ready(output)) => {
                // send result if the JoinHandle is not dropped
                self.finish(cur, output);
            }

            Ok(Poll::Pending) => match self.header().state.turning_to_idle() {
                StateAction::Enqueue => {
                    self.get_scheduled(true);
                }
                StateAction::Failed(state) => panic!("task state invalid: {state}"),
                StateAction::Canceled(state) => {
                    let output = self.get_canceled();
                    self.finish(state, Err(output));
                }
                _ => {}
            },

            Err(_) => {
                let output = Err(ScheduleError::new(ErrorKind::Panic, "panic happen"));
                self.finish(cur, output);
            }
        }
    }

    pub(crate) unsafe fn shutdown(self) {
        // Check if the JoinHandle gets dropped already. If JoinHandle is still there,
        // wakes the JoinHandle.
        let cur = self.header().state.dec_ref();
        if state::ref_count(cur) > 0 && state::is_care_join_handle(cur) {
            self.set_canceled();
        } else {
            self.release();
        }
    }

    pub(crate) fn wake(self) {
        self.wake_by_ref();
        self.drop_ref();
    }

    pub(crate) fn wake_by_ref(&self) {
        let prev = self.header().state.turn_to_scheduling();
        if state::need_enqueue(prev) {
            self.get_scheduled(false);
        }
    }

    // Actually cancels the task during running
    fn get_canceled(&self) -> ScheduleError {
        self.inner().turning_to_used_data();
        ErrorKind::TaskCanceled.into()
    }

    // Sets task state into canceled and scheduled
    pub(crate) fn set_canceled(&self) {
        if self.header().state.turn_to_canceled_and_scheduled() {
            self.get_scheduled(false);
        }
    }

    fn to_task(&self) -> Task {
        unsafe { Task::from_raw(self.header().into()) }
    }

    fn get_scheduled(&self, lifo: bool) {
        // the scheduler must exist when calling this method
        self.inner()
            .scheduler
            .upgrade()
            .expect("the scheduler has already been dropped")
            .schedule(self.to_task(), lifo);
    }
}

#[cfg(feature = "ffrt")]
impl<T, S> TaskHandle<T, S>
where
    T: Future,
    S: Schedule,
{
    pub(crate) fn ffrt_run(self) -> bool {
        self.inner().get_task_ctx();

        match self.header().state.turning_to_running() {
            StateAction::Failed(state) => panic!("turning to running failed: {:b}", state),
            StateAction::Canceled(cur) => {
                let output = self.ffrt_get_canceled();
                self.finish(cur, Err(output));
                return true;
            }
            _ => {}
        }

        let waker = WakerRefHeader::<'_>::new::<T>(self.header());
        let mut context = Context::from_waker(&waker);

        let res = panic::catch_unwind(panic::AssertUnwindSafe(|| {
            self.inner().poll(&mut context).map(Ok)
        }));

        let cur = self.header().state.get_current_state();
        match res {
            Ok(Poll::Ready(output)) => {
                // send result if the JoinHandle is not dropped
                self.finish(cur, output);
                true
            }

            Ok(Poll::Pending) => match self.header().state.turning_to_idle() {
                StateAction::Enqueue => {
                    let ffrt_task = unsafe { (*self.inner().task.get()).as_ref().unwrap() };
                    ffrt_task.wake_task();
                    false
                }
                StateAction::Failed(state) => panic!("task state invalid: {:b}", state),
                StateAction::Canceled(state) => {
                    let output = self.ffrt_get_canceled();
                    self.finish(state, Err(output));
                    true
                }
                _ => false,
            },

            Err(_) => {
                let output = Err(ScheduleError::new(ErrorKind::Panic, "panic happen"));
                self.finish(cur, output);
                true
            }
        }
    }

    pub(crate) fn ffrt_wake(self) {
        self.ffrt_wake_by_ref();
        self.drop_ref();
    }

    pub(crate) fn ffrt_wake_by_ref(&self) {
        let prev = self.header().state.turn_to_scheduling();
        if state::need_enqueue(prev) {
            let ffrt_task = unsafe { (*self.inner().task.get()).as_ref().unwrap() };
            ffrt_task.wake_task();
        }
    }

    // Actually cancels the task during running
    fn ffrt_get_canceled(&self) -> ScheduleError {
        self.inner().turning_to_used_data();
        ErrorKind::TaskCanceled.into()
    }

    // Sets task state into canceled and scheduled
    pub(crate) fn ffrt_set_canceled(&self) {
        if self.header().state.turn_to_canceled_and_scheduled() {
            let ffrt_task = unsafe { (*self.inner().task.get()).as_ref().unwrap() };
            ffrt_task.wake_task();
        }
    }
}
