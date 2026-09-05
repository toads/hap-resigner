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

cfg_not_ffrt!(
    cfg_event!(
        use std::io;
        use std::sync::Arc;
        use crate::executor::worker::get_current_handle;
    );

    cfg_time! {
        use std::fmt::Error;
        use std::ptr::NonNull;
        use crate::time::{Clock, TimeHandle};
        use std::time::Instant;
    }
    cfg_net! {
        use crate::util::slab::Ref;
        use crate::net::IoHandle;
        use ylong_io::{Interest, Source};
        use crate::net::ScheduleIO;
    }

    #[cfg(target_family = "unix")]
    cfg_signal! {
        use ylong_io::Token;
    }

    pub(crate) struct Handle {
        #[cfg(feature = "net")]
        pub(crate) io: IoHandle,
        #[cfg(feature = "time")]
        pub(crate) time: TimeHandle,
    }

    impl Handle {
        pub(crate) fn wake(&self) {
            #[cfg(feature = "net")]
            self.io.waker.wake().unwrap_or_else(|e| panic!("ylong_io wake failed, error: {e}"));
        }

        #[cfg(any(feature = "net", feature = "time"))]
        pub(crate) fn get_handle() -> io::Result<Arc<Handle>> {
            let context = get_current_handle()
                .ok_or_else(|| io::Error::new(io::ErrorKind::Other, "get_current_ctx() fail"))?;
            Ok(context._handle.clone())
        }
    }

    #[cfg(feature = "net")]
    impl Handle {
        pub(crate) fn io_register(
            &self,
            io: &mut impl Source,
            interest: Interest,
        ) -> io::Result<Ref<ScheduleIO>> {
            self.io.register_source(io, interest)
        }

        pub(crate) fn io_deregister(&self, io: &mut impl Source) -> io::Result<()> {
            self.io.deregister_source(io)
        }

        #[cfg(feature = "metrics")]
        pub(crate) fn get_registered_count(&self) -> u64 {
            self.io.get_registered_count()
        }

        #[cfg(feature = "metrics")]
        pub(crate) fn get_ready_count(&self) -> u64 {
            self.io.get_ready_count()
        }
    }

    #[cfg(feature = "time")]
    impl Handle {
        pub(crate) fn start_time(&self) -> Instant {
            self.time.start_time()
        }

        pub(crate) fn timer_register(&self, clock_entry: NonNull<Clock>) -> Result<u64, Error> {
            let res = self.time.timer_register(clock_entry);
            self.wake();
            res
        }

        pub(crate) fn timer_cancel(&self, clock_entry: NonNull<Clock>) {
            self.time.timer_cancel(clock_entry);
        }
    }

    #[cfg(all(feature = "signal", target_family = "unix"))]
    impl Handle {
        pub(crate) fn io_register_with_token(
            &self,
            io: &mut impl Source,
            token: Token,
            interest: Interest,
        ) -> io::Result<()> {
            self.io.register_source_with_token(io, token, interest)
        }
    }
);

cfg_ffrt! {
    use std::sync::Arc;
    use std::io;

    use crate::util::slab::Ref;
    use crate::net::IoHandle;
    use crate::net::ScheduleIO;

    use ylong_io::{Interest, Source};

    pub(crate) struct Handle {
        io: &'static IoHandle,
    }

    impl Handle {
        pub(crate) fn get_handle() -> io::Result<Arc<Handle>> {
            Ok(Arc::new(Handle{io: IoHandle::get_ref()}))
        }

        pub(crate) fn io_register(
            &self,
            io: &mut impl Source,
            interest: Interest,
        ) -> io::Result<Ref<ScheduleIO>> {
            self.io.register_source(io, interest)
        }

        pub(crate) fn io_deregister(&self, io: &mut impl Source) -> io::Result<()> {
            unsafe {
                ylong_ffrt::ffrt_poller_deregister(io.get_fd() as libc::c_int);
            }
            Ok(())
        }
    }
}
