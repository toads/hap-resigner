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

use ylong_runtime::error::{ErrorKind, ScheduleError};

/// SDV test cases for ScheduleError `Debug` and `Display`
///
/// # Brief
/// 1. Creating simple errors
/// 2. Creating complex errors
#[test]
fn sdv_schedule_error_format() {
    let simple_error: ScheduleError = ErrorKind::TaskShutdown.into();
    let custom_error = ScheduleError::new(ErrorKind::TaskShutdown, "task shutdown");

    assert_eq!(format!("{simple_error:?}"), "Kind(TaskShutdown)");
    assert_eq!(
        format!("{custom_error:?}"),
        "Custom { kind: TaskShutdown, error: \"task shutdown\" }"
    );

    assert_eq!(format!("{simple_error}"), "task already get shutdown");
    assert_eq!(format!("{custom_error}"), "TaskShutdown: task shutdown");
}

/// SDV test cases for ScheduleError::new()
///
/// # Brief
/// 1. Creating simple errors
/// 2. Creating complex errors
#[test]
fn sdv_schedule_error_new() {
    let custom_error_one =
        ScheduleError::new(ErrorKind::Other, std::sync::mpsc::RecvTimeoutError::Timeout);
    assert_eq!(
        format!("{custom_error_one:?}"),
        "Custom { kind: Other, error: Timeout }"
    );
    assert_eq!(
        format!("{custom_error_one}"),
        "Other: timed out waiting on channel"
    );

    let custom_error_two = ScheduleError::new(ErrorKind::TaskShutdown, "task shutdown");
    assert_eq!(
        format!("{custom_error_two:?}"),
        "Custom { kind: TaskShutdown, error: \"task shutdown\" }"
    );
    assert_eq!(format!("{custom_error_two}"), "TaskShutdown: task shutdown");
}

/// SDV test cases for ScheduleError::kind()
///
/// # Brief
/// 1. Creating simple errors
/// 2. Creating complex errors
#[test]
fn sdv_schedule_error_kind() {
    let simple_error: ScheduleError = ErrorKind::Other.into();
    let custom_error = ScheduleError::new(ErrorKind::TaskShutdown, "task shutdown");

    assert_eq!(format!("{:?}", simple_error.kind()), "Other");
    assert_eq!(format!("{:?}", custom_error.kind()), "TaskShutdown");
}

/// SDV test cases for ScheduleError::into_inner()
///
/// # Brief
/// 1. Creating simple errors
/// 2. Creating complex errors
#[test]
fn sdv_schedule_error_into_inner() {
    let simple_error: ScheduleError = ErrorKind::Other.into();
    let custom_error = ScheduleError::new(ErrorKind::TaskShutdown, "task shutdown");

    assert!(simple_error.into_inner().is_none());
    assert_eq!(
        format!("{}", custom_error.into_inner().unwrap()),
        "task shutdown"
    );
}
