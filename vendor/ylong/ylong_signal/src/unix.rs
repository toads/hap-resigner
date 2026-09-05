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

//! Linux wrapping of signal syscall

use std::{io, mem, ptr};

use libc::{c_int, c_void, sigaction, siginfo_t};

use crate::common::{SigAction, Signal};
use crate::sig_map::SigMap;

impl SigAction {
    pub(crate) fn get_old_action(sig_num: c_int) -> io::Result<Self> {
        let mut old_act: libc::sigaction = unsafe { mem::zeroed() };
        unsafe {
            if libc::sigaction(sig_num, ptr::null(), &mut old_act) != 0 {
                return Err(io::Error::last_os_error());
            }
        }
        Ok(SigAction {
            sig_num,
            act: old_act,
        })
    }
}

impl Signal {
    pub(crate) fn replace_sigaction(sig_num: c_int, new_action: usize) -> io::Result<sigaction> {
        let mut handler: libc::sigaction = unsafe { mem::zeroed() };
        let mut old_act: libc::sigaction = unsafe { mem::zeroed() };

        handler.sa_sigaction = new_action;
        handler.sa_flags = libc::SA_RESTART | libc::SA_SIGINFO;

        unsafe {
            if libc::sigaction(sig_num, &handler, &mut old_act) != 0 {
                return Err(io::Error::last_os_error());
            }
        }

        Ok(old_act)
    }
}

pub(crate) extern "C" fn sig_handler(sig_num: c_int, sig_info: *mut siginfo_t, data: *mut c_void) {
    let sig_map = SigMap::get_instance();
    let race_fallback = sig_map.race_old.read();
    let signals = sig_map.data.read();

    if let Some(signal) = signals.get(&sig_num) {
        // sig_info should not be null, but in a sig handler we cannot panic directly,
        // therefore we abort instead
        if sig_info.is_null() {
            unsafe { libc::abort() };
        }

        let info = unsafe { &*sig_info };
        if let Some(act) = &signal.new_act {
            act(info);
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
            execute_act(&fallback.act, sig_num, sig_info, data);
        }
    }
}

fn execute_act(act: &sigaction, sig_num: c_int, sig_info: *mut siginfo_t, data: *mut c_void) {
    let handler = act.sa_sigaction;

    // SIG_DFL for the default action.
    // SIG_IGN to ignore this signal.
    if handler == libc::SIG_DFL || handler == libc::SIG_IGN {
        return;
    }

    // If SA_SIGINFO flag is set, then the signal handler takes three arguments, not
    // one. In this case, sa_sigaction should be set instead of sa_handler.
    // We transmute the handler from ptr to actual function type according to
    // definition.
    if act.sa_flags & libc::SA_SIGINFO == 0 {
        let action = unsafe { mem::transmute::<usize, extern "C" fn(c_int)>(handler) };
        action(sig_num);
    } else {
        type Action = extern "C" fn(c_int, *mut siginfo_t, *mut c_void);
        let action = unsafe { mem::transmute::<usize, Action>(handler) };
        action(sig_num, sig_info, data);
    }
}
