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
use std::io::{Error, ErrorKind, Write};
use std::mem::MaybeUninit;
use std::os::raw::c_int;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering::{Acquire, Release, SeqCst};
use std::sync::Once;

use ylong_io::UnixStream;
use ylong_signal::register_signal_action;

use crate::signal::SignalKind;
use crate::sync::watch::{channel, Receiver, Sender};

pub(crate) struct Event {
    inner: Sender<()>,
    notify: AtomicBool,
    once: Once,
    is_registered: AtomicBool,
}

impl Default for Event {
    fn default() -> Self {
        let (tx, _) = channel(());
        Self {
            inner: tx,
            notify: AtomicBool::new(false),
            once: Once::new(),
            is_registered: AtomicBool::new(false),
        }
    }
}

impl Event {
    pub(crate) fn register<F>(&self, signal_kind: c_int, f: F) -> io::Result<()>
    where
        F: Fn() + Sync + Send + 'static,
    {
        let mut register_res = Ok(());
        self.once.call_once(|| {
            register_res = unsafe { register_signal_action(signal_kind, f) };
            if register_res.is_ok() {
                self.is_registered.store(true, Release);
            }
        });
        register_res?;
        if self.is_registered.load(Acquire) {
            Ok(())
        } else {
            Err(Error::new(ErrorKind::Other, "Failed to register signal"))
        }
    }
}

struct SignalStream {
    sender: UnixStream,
    receiver: UnixStream,
}

impl Default for SignalStream {
    fn default() -> Self {
        let (sender, receiver) = UnixStream::pair()
            .unwrap_or_else(|e| panic!("failed to create a pair of UnixStream, error: {e}"));
        Self { sender, receiver }
    }
}

pub(crate) struct Registry {
    stream: SignalStream,
    events: Vec<Event>,
}

impl Default for Registry {
    fn default() -> Self {
        Self {
            stream: SignalStream::default(),
            events: (0..=SignalKind::get_max())
                .map(|_| Event::default())
                .collect(),
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

    pub(crate) fn get_event(&self, event_id: usize) -> &Event {
        // Invalid signal kinds have been forbidden, the scope of signal kinds has been
        // protected.
        self.events
            .get(event_id)
            .unwrap_or_else(|| panic!("invalid event_id: {}", event_id))
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

    pub(crate) fn notify_event(&self, event_id: usize) {
        if let Some(event) = self.events.get(event_id) {
            event.notify.store(true, SeqCst);
        }
    }

    pub(crate) fn broadcast(&self) {
        for event in &self.events {
            if event.notify.swap(false, SeqCst) {
                let _ = event.inner.send(());
            }
        }
    }

    pub(crate) fn write(&self, buf: &[u8]) -> io::Result<usize> {
        let mut sender = &self.stream.sender;
        sender.write(buf)
    }

    pub(crate) fn try_clone_stream(&self) -> io::Result<UnixStream> {
        self.stream.receiver.try_clone()
    }
}
