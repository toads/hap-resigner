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

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

pub(crate) struct Sleeper {
    record: Record,
    idle_list: Mutex<Vec<usize>>,
    num_workers: usize,
    pub(crate) wake_by_search: Mutex<Vec<bool>>,
}

impl Sleeper {
    pub fn new(num_workers: usize) -> Self {
        Sleeper {
            record: Record::new(num_workers),
            idle_list: Mutex::new(Vec::with_capacity(num_workers)),
            num_workers,
            wake_by_search: Mutex::new(vec![false; num_workers]),
        }
    }

    pub fn is_parked(&self, worker_index: &usize) -> bool {
        let idle_list = self.idle_list.lock().unwrap();
        idle_list.contains(worker_index)
    }

    pub fn pop_worker_by_id(&self, worker_index: &usize) {
        let mut idle_list = self.idle_list.lock().unwrap();

        for i in 0..idle_list.len() {
            if &idle_list[i] == worker_index {
                idle_list.swap_remove(i);
                self.record.inc_active_num();
                break;
            }
        }
    }

    pub fn pop_worker(&self, last_search: bool) -> Option<usize> {
        let (active_num, searching_num) = self.record.load_state();
        if active_num >= self.num_workers || searching_num > 0 {
            return None;
        }

        let mut idle_list = self.idle_list.lock().unwrap();

        let res = idle_list.pop();
        drop(idle_list);
        if let Some(worker_idx) = res.as_ref() {
            if last_search {
                let mut search_list = self.wake_by_search.lock().unwrap();
                search_list[*worker_idx] = true;
            }
            self.record.inc_active_num();
        }

        res
    }

    // return true if it's the last thread going to sleep.
    pub fn push_worker(&self, worker_index: usize) -> bool {
        let mut idle_list = self.idle_list.lock().unwrap();

        idle_list.push(worker_index);
        self.record.dec_active_num()
    }

    #[inline]
    pub fn inc_searching_num(&self) {
        self.record.inc_searching_num();
    }

    pub fn try_inc_searching_num(&self) -> bool {
        let (active_num, searching_num) = self.record.load_state();

        if searching_num * 2 < active_num {
            // increment searching worker number
            self.inc_searching_num();
            return true;
        }
        false
    }

    // return true if it's the last searching thread
    #[inline]
    pub fn dec_searching_num(&self) -> bool {
        self.record.dec_searching_num()
    }
}

const ACTIVE_WORKER_SHIFT: usize = 16;
const SEARCHING_MASK: usize = (1 << ACTIVE_WORKER_SHIFT) - 1;
const ACTIVE_MASK: usize = !SEARCHING_MASK;
//        32 bits          16 bits       16 bits
// |-------------------| working num | searching num|
struct Record(AtomicUsize);

impl Record {
    fn new(active_num: usize) -> Self {
        Self(AtomicUsize::new(active_num << ACTIVE_WORKER_SHIFT))
    }

    // Return true if it is the last searching thread
    fn dec_searching_num(&self) -> bool {
        let ret = self.0.fetch_sub(1, Ordering::SeqCst);
        (ret & SEARCHING_MASK) == 1
    }

    fn inc_searching_num(&self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }

    fn inc_active_num(&self) {
        let inc = 1 << ACTIVE_WORKER_SHIFT;

        self.0.fetch_add(inc, Ordering::SeqCst);
    }

    fn dec_active_num(&self) -> bool {
        let dec = 1 << ACTIVE_WORKER_SHIFT;

        let ret = self.0.fetch_sub(dec, Ordering::SeqCst);
        let active_num = ((ret & ACTIVE_MASK) >> ACTIVE_WORKER_SHIFT) - 1;
        active_num == 0
    }

    fn load_state(&self) -> (usize, usize) {
        let union_num = self.0.load(Ordering::SeqCst);

        let searching_num = union_num & SEARCHING_MASK;
        let active_num = (union_num & ACTIVE_MASK) >> ACTIVE_WORKER_SHIFT;

        (active_num, searching_num)
    }
}
