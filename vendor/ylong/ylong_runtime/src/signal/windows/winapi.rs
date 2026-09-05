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

#![allow(non_camel_case_types)]
#![allow(clippy::upper_case_acronyms)]

pub type BOOL = i32;
pub type PHANDLER_ROUTINE = Option<unsafe extern "system" fn(ctrlType: u32) -> BOOL>;

pub const CTRL_C_EVENT: u32 = 0u32;
pub const CTRL_BREAK_EVENT: u32 = 1u32;
pub const CTRL_CLOSE_EVENT: u32 = 2u32;
pub const CTRL_LOGOFF_EVENT: u32 = 5u32;
pub const CTRL_SHUTDOWN_EVENT: u32 = 6u32;

extern "system" {
    pub fn SetConsoleCtrlHandler(HandlerRoutine: PHANDLER_ROUTINE, Add: BOOL) -> BOOL;
}
