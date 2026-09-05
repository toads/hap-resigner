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

//! windows wrapping of signal syscall

use std::{io, mem};

use libc::{c_int, sighandler_t, SIGFPE, SIG_DFL, SIG_ERR, SIG_GET, SIG_IGN};

use crate::common::{siginfo_t, SigAction, Signal};
use crate::sig_map::SigMap;

impl SigAction {
    pub(crate) fn get_old_action(sig_num: c_int) -> io::Result<Self> {
        let old_act = unsafe { libc::signal(sig_num, SIG_GET) };
        if old_act == SIG_ERR as sighandler_t {
            return Err(io::Error::last_os_error());
        }
        Ok(SigAction {
            sig_num,
            act: old_act,
        })
    }
}

impl Signal {
    pub(crate) fn replace_sigaction(
        sig_num: c_int,
        new_action: sighandler_t,
    ) -> io::Result<sighandler_t> {
        let old_act = unsafe { libc::signal(sig_num, new_action) };

        if old_act == SIG_ERR as sighandler_t {
            return Err(io::Error::last_os_error());
        }

        Ok(old_act)
    }
}

pub(crate) extern "C" fn sig_handler(sig_num: c_int) {
    if sig_num != SIGFPE {
        let old = unsafe { libc::signal(sig_num, sig_handler as usize) };
        if old == SIG_ERR as sighandler_t {
            unsafe {
                libc::abort();
            }
        }
    }

    let sig_map = SigMap::get_instance();
    let race_fallback = sig_map.race_old.read();
    let data = sig_map.data.read();

    if let Some(signal) = data.get(&sig_num) {
        if let Some(act) = &signal.new_act {
            act(&siginfo_t);
        }
    } else if let Some(fallback) = race_fallback.as_ref() {
        // There could be a race condition between swapping the old handler with the new
        // handler and storing the change back to the global during the register
        // procedure. Because of the race condition, the old handler and the new
        // action could both not get executed. In order to prevent this, we
        // store the old handler into global before swapping the handler in
        // register. And during the handler execution, if the the action
        // of the signal cannot be found, we execute this old handler instead if the
        // sig_num matches.
        if fallback.sig_num == sig_num {
            execute_act(fallback.act, sig_num);
        }
    }
}

fn execute_act(act: sighandler_t, sig_num: c_int) {
    if act != 0 && act != SIG_DFL && act != SIG_IGN {
        unsafe {
            let action = mem::transmute::<usize, extern "C" fn(c_int)>(act);
            action(sig_num);
        }
    }
}
