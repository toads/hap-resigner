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

macro_rules! cfg_net {
    ($($item:item)*) => {
        $(
            #[cfg(feature = "net")]
            $item
        )*
    }
}

macro_rules! cfg_time {
    ($($item:item)*) => {
        $(
            #[cfg(feature = "time")]
            $item
        )*
    }
}

macro_rules! cfg_ffrt {
    ($($item:item)*) => {
        $(
            #[cfg(feature = "ffrt")]
            $item
        )*
    }
}

macro_rules! cfg_signal {
    ($($item:item)*) => {
        $(
            #[cfg(feature = "signal")]
            $item
        )*
    }
}

#[cfg(target_os = "linux")]
macro_rules! cfg_process {
    ($($item:item)*) => {
        $(
            #[cfg(feature = "process")]
            $item
        )*
    }
}

macro_rules! cfg_sync {
    ($($item:item)*) => {
        $(
            #[cfg(feature = "sync")]
            $item
        )*
    }
}

macro_rules! cfg_macros {
    ($($item:item)*) => {
        $(
            #[cfg(feature = "macros")]
            $item
        )*
    }
}

macro_rules! cfg_fs {
    ($($item:item)*) => {
        $(
            #[cfg(feature = "fs")]
            $item
        )*
    }
}

#[cfg(not(feature = "ffrt"))]
macro_rules! cfg_event {
    ($($item:item)*) => {
        $(
            #[cfg(any(feature = "net", feature = "time"))]
            $item
        )*
    }
}

macro_rules! cfg_not_ffrt {
    ($($item:item)*) => {
        $(
            #[cfg(not(feature = "ffrt"))]
            $item
        )*
    }
}

macro_rules! cfg_metrics {
    ($($item:item)*) => {
        $(
            #[cfg(feature = "metrics")]
            $item
        )*
    }
}

macro_rules! cfg_unix {
    ($($item:item)*) => {
        $(
            #[cfg(unix)]
            $item
        )*
    }
}

macro_rules! cfg_windows {
    ($($item:item)*) => {
        $(
            #[cfg(windows)]
            $item
        )*
    }
}

#[cfg(all(target_os = "linux", any(feature = "net", feature = "process")))]
macro_rules! syscall {
    ($fn: ident ( $($arg: expr),* $(,)* ) ) => {{
        let res = unsafe { libc::$fn($($arg, )*) };
        if res == -1 {
            Err(std::io::Error::last_os_error())
        } else {
            Ok(res)
        }
    }};
}
