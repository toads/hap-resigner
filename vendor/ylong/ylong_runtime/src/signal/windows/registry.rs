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

use std::mem::MaybeUninit;
use std::sync::Once;

use crate::sync::watch::{channel, Receiver, Sender};

const EVENT_MAX_NUM: usize = 6;

pub(crate) struct Event {
    inner: Sender<()>,
}

impl Default for Event {
    fn default() -> Self {
        let (tx, _) = channel(());
        Self { inner: tx }
    }
}

pub(crate) struct Registry {
    events: Vec<Event>,
}

impl Default for Registry {
    fn default() -> Self {
        Self {
            events: (0..=EVENT_MAX_NUM).map(|_| Event::default()).collect(),
        }
    }
}

impl Registry {
    pub(crate) fn get_instance() -> &'static Registry {
        static mut REGISTRY: MaybeUninit<Registry> = MaybeUninit::uninit();
        static REGISTRY_ONCE: Once = Once::new();
        unsafe {
            REGISTRY_ONCE.call_once(|| {
                REGISTRY = MaybeUninit::new(Registry::default());
            });
            &*REGISTRY.as_ptr()
        }
    }

    pub(crate) fn listen_to_event(&self, event_id: usize) -> Receiver<()> {
        // Invalid signal kinds have been forbidden, the scope of signal kinds has been
        // protected.
        self.events
            .get(event_id)
            .unwrap_or_else(|| panic!("invalid event_id: {}", event_id))
            .inner
            .subscribe()
    }

    pub(crate) fn broadcast(&self, event_id: usize) -> i32 {
        if let Some(event) = self.events.get(event_id) {
            if event.inner.send(()).is_ok() {
                return 1;
            }
        }
        0
    }
}
