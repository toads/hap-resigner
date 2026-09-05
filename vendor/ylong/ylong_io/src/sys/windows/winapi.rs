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
#![allow(non_snake_case)]
#![allow(clippy::upper_case_acronyms)]

use libc::{c_char, c_int, c_long, c_uchar, c_ulong, c_ushort, c_void};

pub type HANDLE = isize;
pub type DWORD = c_ulong;
pub type ULONG_PTR = usize;
pub type BOOL = c_int;
pub type ULONG = c_ulong;
pub type PULONG = *mut ULONG;
pub type UCHAR = c_uchar;
pub type PVOID = *mut c_void;
pub type ADDRESS_FAMILY = USHORT;
pub type CHAR = c_char;
pub type LPOVERLAPPED = *mut OVERLAPPED;
pub type LPOVERLAPPED_ENTRY = *mut OVERLAPPED_ENTRY;
pub type NTSTATUS = c_long;
pub type PWSTR = *mut u16;
pub type PUNICODE_STRING = *mut UNICODE_STRING;
pub type USHORT = c_ushort;
pub type WIN32_ERROR = u32;
pub type WSA_ERROR = i32;
pub type SOCKET = usize;
pub type FILE_SHARE_MODE = u32;
pub type NT_CREATE_FILE_DISPOSITION = u32;
pub type FILE_ACCESS_FLAGS = u32;
pub type PIO_APC_ROUTINE = Option<
    unsafe extern "system" fn(
        apcContext: *mut c_void,
        ioStatusBlock: *mut IO_STATUS_BLOCK,
        reserved: u32,
    ) -> (),
>;
pub type LPWSAOVERLAPPED_COMPLETION_ROUTINE = Option<
    unsafe extern "system" fn(
        dwError: u32,
        cbTransferred: u32,
        lpOverlapped: *mut OVERLAPPED,
        dwFlags: u32,
    ) -> (),
>;

pub const AF_INET: ADDRESS_FAMILY = 2;
pub const AF_INET6: ADDRESS_FAMILY = 23;

pub const FIONBIO: c_long = -2147195266;
pub const INVALID_SOCKET: SOCKET = -1 as _;

pub const SIO_BASE_HANDLE: u32 = 1207959586;
pub const SIO_BSP_HANDLE: u32 = 1207959579;
pub const SIO_BSP_HANDLE_POLL: u32 = 1207959581;
pub const SIO_BSP_HANDLE_SELECT: u32 = 1207959580;

pub const SOCKET_ERROR: i32 = -1;
pub const SOCK_DGRAM: u16 = 2u16;

pub const INVALID_HANDLE_VALUE: HANDLE = -1i32 as _;
pub const STATUS_NOT_FOUND: NTSTATUS = -1073741275;
pub const STATUS_PENDING: NTSTATUS = 259;
pub const STATUS_SUCCESS: NTSTATUS = 0;
pub const STATUS_CANCELLED: NTSTATUS = -1073741536;

pub const SOCK_STREAM: u16 = 1u16;
pub const SOL_SOCKET: u32 = 65535u32;
pub const SO_LINGER: u32 = 128u32;

pub const FILE_OPEN: NT_CREATE_FILE_DISPOSITION = 1;
pub const FILE_SHARE_READ: FILE_SHARE_MODE = 1;
pub const FILE_SHARE_WRITE: FILE_SHARE_MODE = 2;

pub const SYNCHRONIZE: FILE_ACCESS_FLAGS = 1048576;
pub const FILE_SKIP_SET_EVENT_ON_HANDLE: u32 = 2;

pub const ERROR_INVALID_HANDLE: WIN32_ERROR = 6;
pub const ERROR_IO_PENDING: WIN32_ERROR = 997;
pub const WAIT_TIMEOUT: WIN32_ERROR = 258;

macro_rules! impl_clone {
    ($name: ident) => {
        impl Clone for $name {
            fn clone(&self) -> $name {
                *self
            }
        }
    };
}

extern "system" {
    pub fn CloseHandle(hObject: HANDLE) -> BOOL;

    pub fn CreateIoCompletionPort(
        FileHandle: HANDLE,
        ExistingCompletionPort: HANDLE,
        CompletionKey: ULONG_PTR,
        NumberOfConcurrentThreads: DWORD,
    ) -> HANDLE;

    pub fn PostQueuedCompletionStatus(
        CompletionPort: HANDLE,
        dwNumberOfBytesTransferred: DWORD,
        dwCompletionKey: ULONG_PTR,
        lpOverlapped: LPOVERLAPPED,
    ) -> BOOL;

    pub fn GetQueuedCompletionStatusEx(
        CompletionPort: HANDLE,
        lpCompletionPortEntries: LPOVERLAPPED_ENTRY,
        ulCount: ULONG,
        ulNumEntriesRemoved: PULONG,
        dwMilliseconds: DWORD,
        fAlertable: BOOL,
    ) -> BOOL;

    pub fn RtlNtStatusToDosError(status: NTSTATUS) -> u32;

    pub fn NtCreateFile(
        FileHandle: *mut HANDLE,
        DesiredAccess: ULONG,
        ObjectAttributes: *mut OBJECT_ATTRIBUTES,
        IoStatusBlock: *mut IO_STATUS_BLOCK,
        AllocationSize: *mut i64,
        FileAttributes: ULONG,
        ShareAccess: FILE_SHARE_MODE,
        CreateDisposition: NT_CREATE_FILE_DISPOSITION,
        CreateOptions: ULONG,
        EaBuffer: PVOID,
        EaLength: ULONG,
    ) -> NTSTATUS;

    pub fn NtDeviceIoControlFile(
        FileHandle: HANDLE,
        Event: HANDLE,
        ApcRoutine: PIO_APC_ROUTINE,
        ApcContext: PVOID,
        IoStatusBlock: *mut IO_STATUS_BLOCK,
        IoControlCode: ULONG,
        InputBuffer: PVOID,
        InputBufferLength: ULONG,
        OutputBuffer: PVOID,
        OutputBufferLength: ULONG,
    ) -> NTSTATUS;

    pub fn SetFileCompletionNotificationModes(FileHandle: HANDLE, Flags: UCHAR) -> BOOL;

    pub fn WSAGetLastError() -> WSA_ERROR;

    pub fn WSAIoctl(
        s: SOCKET,
        dwIoControlCode: u32,
        lpvInBuffer: *const c_void,
        cbInBuffer: u32,
        lpvOutBuffer: *mut c_void,
        cbOutBuffer: u32,
        lpcbBytesReturned: *mut u32,
        lpOverlapped: *mut OVERLAPPED,
        lpCompletionRoutine: LPWSAOVERLAPPED_COMPLETION_ROUTINE,
    ) -> c_int;

    pub fn bind(s: SOCKET, name: *const SOCKADDR, namelen: c_int) -> c_int;

    pub fn closesocket(s: SOCKET) -> c_int;

    pub fn ioctlsocket(s: SOCKET, cmd: c_long, argp: *mut c_ulong) -> c_int;

    pub fn socket(af: c_int, _type: c_int, protocol: c_int) -> SOCKET;

    pub fn setsockopt(
        s: SOCKET,
        level: c_int,
        optname: c_int,
        optval: *const c_char,
        optlen: c_int,
    ) -> c_int;

    pub fn listen(s: SOCKET, backlog: c_int) -> c_int;

    pub fn connect(s: SOCKET, name: *const SOCKADDR, namelen: c_int) -> c_int;
}

#[repr(C)]
#[derive(Copy)]
pub struct OVERLAPPED {
    pub Internal: ULONG_PTR,
    pub InternalHigh: ULONG_PTR,
    pub Anonymous: OVERLAPPED_0,
    pub hEvent: HANDLE,
}

#[repr(C)]
#[derive(Copy)]
pub union OVERLAPPED_0 {
    pub Anonymous: OVERLAPPED_0_0,
    pub Pointer: *mut c_void,
}

#[repr(C)]
#[derive(Copy)]
pub struct OVERLAPPED_0_0 {
    pub Offset: u32,
    pub OffsetHigh: u32,
}

#[repr(C)]
#[derive(Copy)]
pub struct OVERLAPPED_ENTRY {
    pub lpCompletionKey: ULONG_PTR,
    pub lpOverlapped: LPOVERLAPPED,
    pub Internal: ULONG_PTR,
    pub dwNumberOfBytesTransferred: DWORD,
}

#[repr(C)]
#[derive(Copy)]
pub struct UNICODE_STRING {
    pub Length: USHORT,
    pub MaximumLength: USHORT,
    pub Buffer: PWSTR,
}

#[repr(C)]
#[derive(Copy)]
pub struct OBJECT_ATTRIBUTES {
    pub Length: ULONG,
    pub RootDirectory: HANDLE,
    pub ObjectName: PUNICODE_STRING,
    pub Attributes: ULONG,
    pub SecurityDescriptor: PVOID,
    pub SecurityQualityOfService: PVOID,
}

#[repr(C)]
#[derive(Copy)]
pub struct IO_STATUS_BLOCK {
    pub Anonymous: IO_STATUS_BLOCK_0,
    pub Information: usize,
}

#[repr(C)]
#[derive(Copy)]
pub union IO_STATUS_BLOCK_0 {
    pub Status: NTSTATUS,
    pub Pointer: *mut c_void,
}

#[repr(C)]
#[derive(Copy)]
pub struct SOCKADDR {
    sa_family: ADDRESS_FAMILY,
    sa_data: [CHAR; 14],
}

#[repr(C)]
#[derive(Copy)]
pub struct IN6_ADDR {
    pub u: IN6_ADDR_0,
}

#[repr(C)]
#[derive(Copy)]
pub union IN6_ADDR_0 {
    pub Byte: [u8; 16],
    pub Word: [u16; 8],
}

#[repr(C)]
#[derive(Copy)]
pub struct IN_ADDR {
    pub S_un: IN_ADDR_0,
}

#[repr(C)]
#[derive(Copy)]
pub union IN_ADDR_0 {
    pub S_un_b: IN_ADDR_0_0,
    pub S_un_w: IN_ADDR_0_1,
    pub S_addr: u32,
}

#[repr(C)]
#[derive(Copy)]
pub struct IN_ADDR_0_0 {
    pub s_b1: u8,
    pub s_b2: u8,
    pub s_b3: u8,
    pub s_b4: u8,
}

#[repr(C)]
#[derive(Copy)]
pub struct IN_ADDR_0_1 {
    pub s_w1: u16,
    pub s_w2: u16,
}

#[repr(C)]
#[derive(Copy)]
pub struct SOCKADDR_IN {
    pub sin_family: ADDRESS_FAMILY,
    pub sin_port: u16,
    pub sin_addr: IN_ADDR,
    pub sin_zero: [CHAR; 8],
}

#[repr(C)]
#[derive(Copy)]
pub struct SOCKADDR_IN6 {
    pub sin6_family: ADDRESS_FAMILY,
    pub sin6_port: u16,
    pub sin6_flowinfo: u32,
    pub sin6_addr: IN6_ADDR,
    pub Anonymous: SOCKADDR_IN6_0,
}

#[repr(C)]
#[derive(Copy)]
pub union SOCKADDR_IN6_0 {
    pub sin6_scope_id: u32,
    pub sin6_scope_struct: SCOPE_ID,
}

#[repr(C)]
#[derive(Copy)]
pub struct SCOPE_ID {
    pub Anonymous: SCOPE_ID_0,
}

#[repr(C)]
#[derive(Copy)]
pub union SCOPE_ID_0 {
    pub Anonymous: SCOPE_ID_0_0,
    pub Value: u32,
}

#[repr(C)]
#[derive(Copy)]
pub struct SCOPE_ID_0_0 {
    pub _bitfield: u32,
}

#[repr(C)]
#[derive(Copy)]
pub struct LINGER {
    pub l_onoff: u16,
    pub l_linger: u16,
}

impl_clone!(OVERLAPPED);
impl_clone!(OVERLAPPED_0);
impl_clone!(OVERLAPPED_0_0);
impl_clone!(OVERLAPPED_ENTRY);
impl_clone!(UNICODE_STRING);
impl_clone!(OBJECT_ATTRIBUTES);
impl_clone!(IO_STATUS_BLOCK);
impl_clone!(IO_STATUS_BLOCK_0);
impl_clone!(SOCKADDR);
impl_clone!(IN6_ADDR);
impl_clone!(IN6_ADDR_0);
impl_clone!(IN_ADDR);
impl_clone!(IN_ADDR_0);
impl_clone!(IN_ADDR_0_0);
impl_clone!(IN_ADDR_0_1);
impl_clone!(SOCKADDR_IN);
impl_clone!(SOCKADDR_IN6);
impl_clone!(SOCKADDR_IN6_0);
impl_clone!(SCOPE_ID);
impl_clone!(SCOPE_ID_0);
impl_clone!(SCOPE_ID_0_0);
impl_clone!(LINGER);
