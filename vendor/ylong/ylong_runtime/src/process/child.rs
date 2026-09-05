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
use std::io::IoSlice;
#[cfg(unix)]
use std::os::unix::io::{AsFd, AsRawFd, BorrowedFd, OwnedFd, RawFd};
use std::pin::Pin;
use std::process::{Child as StdChild, ExitStatus, Output};
use std::task::{Context, Poll};

use crate::io::{AsyncRead, AsyncReadExt, AsyncWrite, ReadBuf};
use crate::process::sys::ChildStdio;

#[derive(Debug)]
pub(crate) enum ChildState {
    Pending(super::sys::Child),
    Ready(ExitStatus),
}

/// Handle of child process
#[derive(Debug)]
pub struct Child {
    state: ChildState,
    // Weather kill the child when drop
    kill_on_drop: bool,
    /// Options of stdin
    stdin: Option<ChildStdin>,
    /// Options of stdout
    stdout: Option<ChildStdout>,
    /// Options of stderr
    stderr: Option<ChildStderr>,
}

impl Child {
    pub(crate) fn new(
        child: StdChild,
        kill_on_drop: bool,
        stdin: Option<ChildStdin>,
        stdout: Option<ChildStdout>,
        stderr: Option<ChildStderr>,
    ) -> io::Result<Self> {
        Ok(Self {
            state: ChildState::Pending(super::sys::Child::new(child)?),
            kill_on_drop,
            stdin,
            stdout,
            stderr,
        })
    }

    /// Gets the OS-assigned process identifier associated with this child.
    ///
    /// If the child process is exited, it returns `None`.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use ylong_runtime::process::Command;
    ///
    /// fn command() {
    ///     let mut child = Command::new("ls")
    ///         .spawn()
    ///         .expect("ls command failed to start");
    ///     let _id = child.id().expect("the child process is exited");
    /// }
    /// ```
    pub fn id(&self) -> Option<u32> {
        match &self.state {
            ChildState::Pending(child) => Some(child.id()),
            ChildState::Ready(_) => None,
        }
    }

    /// Takes the stdin of this child, remain `None`.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use std::process::Stdio;
    ///
    /// use ylong_runtime::process::Command;
    ///
    /// fn command() {
    ///     let mut child = Command::new("ls")
    ///         .stdin(Stdio::piped())
    ///         .spawn()
    ///         .expect("ls command failed to start");
    ///     let _stdin = child.take_stdin().unwrap();
    /// }
    /// ```
    pub fn take_stdin(&mut self) -> Option<ChildStdin> {
        self.stdin.take()
    }

    /// Takes the stdout of this child, remain `None`.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use std::process::Stdio;
    ///
    /// use ylong_runtime::process::Command;
    ///
    /// fn command() {
    ///     let mut child = Command::new("ls")
    ///         .stdout(Stdio::piped())
    ///         .spawn()
    ///         .expect("ls command failed to start");
    ///     let _stdout = child.take_stdout().unwrap();
    /// }
    /// ```
    pub fn take_stdout(&mut self) -> Option<ChildStdout> {
        self.stdout.take()
    }

    /// Takes the stderr of this child, remain `None`.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use std::process::Stdio;
    ///
    /// use ylong_runtime::process::Command;
    ///
    /// fn command() {
    ///     let mut child = Command::new("ls")
    ///         .stderr(Stdio::piped())
    ///         .spawn()
    ///         .expect("ls command failed to start");
    ///     let _stderr = child.take_stderr().unwrap();
    /// }
    /// ```
    pub fn take_stderr(&mut self) -> Option<ChildStderr> {
        self.stderr.take()
    }

    /// Tries to kill the child process, but doesn't wait for it to take effect.
    ///
    /// On Unix, this is equivalent to sending a SIGKILL. User should ensure
    /// either `child.wait().await` or `child.try_wait()` is invoked
    /// successfully, otherwise the child process will be a Zombie Process.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use std::io;
    /// use std::process::ExitStatus;
    ///
    /// use ylong_runtime::process::Command;
    ///
    /// async fn command() -> io::Result<ExitStatus> {
    ///     let mut child = Command::new("ls")
    ///         .spawn()
    ///         .expect("ls command failed to start");
    ///     child.start_kill().expect("failed to start_kill");
    ///     child.wait().await
    /// }
    /// ```
    pub fn start_kill(&mut self) -> io::Result<()> {
        match &mut self.state {
            ChildState::Pending(ref mut child) => {
                let res = child.kill();
                if res.is_ok() {
                    self.kill_on_drop = false;
                }
                res
            }
            ChildState::Ready(_) => Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "can not kill an exited process",
            )),
        }
    }

    /// Kills the child process and wait().
    ///
    /// # Example
    ///
    /// ```no_run
    /// use ylong_runtime::process::Command;
    ///
    /// async fn command() {
    ///     let mut child = Command::new("ls")
    ///         .spawn()
    ///         .expect("ls command failed to start");
    ///     child.kill().await.expect("failed to kill");
    /// }
    /// ```
    pub async fn kill(&mut self) -> io::Result<()> {
        self.start_kill()?;
        self.wait().await.map(|_| ())
    }

    /// Waits for the child process to exit, and return the status.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use ylong_runtime::process::Command;
    ///
    /// async fn command() {
    ///     let mut child = Command::new("ls")
    ///         .spawn()
    ///         .expect("ls command failed to start");
    ///     let res = child.wait().await.expect("failed to kill");
    /// }
    /// ```
    pub async fn wait(&mut self) -> io::Result<ExitStatus> {
        // take stdin to avoid deadlock.
        drop(self.take_stdin());
        match &mut self.state {
            ChildState::Pending(child) => {
                let res = child.await;

                if let Ok(exit_status) = res {
                    self.kill_on_drop = false;
                    self.state = ChildState::Ready(exit_status);
                }

                res
            }
            ChildState::Ready(exit_status) => Ok(*exit_status),
        }
    }

    /// Tries to get th exit status of the child if it is already exited.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use ylong_runtime::process::Command;
    ///
    /// async fn command() {
    ///     let mut child = Command::new("ls")
    ///         .spawn()
    ///         .expect("ls command failed to start");
    ///     let res = child.try_wait().expect("failed to try_wait!");
    /// }
    /// ```
    pub fn try_wait(&mut self) -> io::Result<Option<ExitStatus>> {
        match &mut self.state {
            ChildState::Pending(child) => {
                let res = child.try_wait();

                if let Ok(Some(exit_status)) = res {
                    // the child is exited, no need for a kill
                    self.kill_on_drop = false;
                    self.state = ChildState::Ready(exit_status);
                }

                res
            }
            ChildState::Ready(exit_status) => Ok(Some(*exit_status)),
        }
    }

    /// Returns the `Output` with exit status, stdout and stderr of child
    /// process.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use ylong_runtime::process::Command;
    ///
    /// async fn command() {
    ///     let mut child = Command::new("ls")
    ///         .spawn()
    ///         .expect("ls command failed to start");
    ///     let res = child.output_wait().await.expect("failed to output_wait");
    /// }
    /// ```
    pub async fn output_wait(&mut self) -> io::Result<Output> {
        async fn read_to_end<T: AsyncRead + Unpin>(io: &mut Option<T>) -> io::Result<Vec<u8>> {
            let mut vec = Vec::new();
            if let Some(io) = io.as_mut() {
                io.read_to_end(&mut vec).await?;
            }
            Ok(vec)
        }

        let mut child_stdout = self.take_stdout();
        let mut child_stderr = self.take_stderr();

        let fut1 = self.wait();
        let fut2 = read_to_end(&mut child_stdout);
        let fut3 = read_to_end(&mut child_stderr);

        let (status, stdout, stderr) =
            crate::process::try_join3::try_join3(fut1, fut2, fut3).await?;

        drop(child_stdout);
        drop(child_stderr);

        Ok(Output {
            status,
            stdout,
            stderr,
        })
    }
}

impl Drop for Child {
    fn drop(&mut self) {
        if self.kill_on_drop {
            if let ChildState::Pending(child) = &mut self.state {
                let _ = child.kill();
            }
        }
    }
}

/// Standard input stream of Child which implements `AsyncWrite` trait
#[derive(Debug)]
pub struct ChildStdin {
    inner: ChildStdio,
}

/// Standard output stream of Child which implements `AsyncRead` trait
#[derive(Debug)]
pub struct ChildStdout {
    inner: ChildStdio,
}

/// Standard err stream of Child which implements `AsyncRead` trait
#[derive(Debug)]
pub struct ChildStderr {
    inner: ChildStdio,
}

impl AsyncWrite for ChildStdin {
    fn poll_write(
        mut self: Pin<&mut Self>,
        ctx: &mut Context<'_>,
        buffer: &[u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.inner).poll_write(ctx, buffer)
    }

    fn poll_write_vectored(
        mut self: Pin<&mut Self>,
        ctx: &mut Context<'_>,
        bufs: &[IoSlice<'_>],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.inner).poll_write_vectored(ctx, bufs)
    }

    fn is_write_vectored(&self) -> bool {
        self.inner.is_write_vectored()
    }

    fn poll_flush(mut self: Pin<&mut Self>, ctx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_flush(ctx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, ctx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_shutdown(ctx)
    }
}

impl AsyncRead for ChildStdout {
    fn poll_read(
        mut self: Pin<&mut Self>,
        ctx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_read(ctx, buf)
    }
}

impl AsyncRead for ChildStderr {
    fn poll_read(
        mut self: Pin<&mut Self>,
        ctx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_read(ctx, buf)
    }
}

macro_rules! impl_common_traits {
    ($ident:ident) => {
        impl $ident {
            pub(crate) fn new(inner: ChildStdio) -> Self {
                Self { inner }
            }

            /// Creates an async `ChildStd` from `std::process::ChildStd`
            pub fn from_std(inner: std::process::$ident) -> io::Result<Self> {
                super::sys::stdio(inner).map(|inner| Self { inner })
            }

            /// Convert to OwnedFd
            #[cfg(unix)]
            pub fn into_owned_fd(self) -> io::Result<OwnedFd> {
                self.inner.into_owned_fd()
            }
        }

        impl TryInto<std::process::Stdio> for $ident {
            type Error = io::Error;

            fn try_into(self) -> Result<std::process::Stdio, Self::Error> {
                super::sys::to_stdio(self.inner)
            }
        }

        #[cfg(unix)]
        impl AsRawFd for $ident {
            fn as_raw_fd(&self) -> RawFd {
                self.inner.as_raw_fd()
            }
        }

        #[cfg(unix)]
        impl AsFd for $ident {
            fn as_fd(&self) -> BorrowedFd<'_> {
                self.inner.as_fd()
            }
        }
    };
}

impl_common_traits!(ChildStdin);
impl_common_traits!(ChildStdout);
impl_common_traits!(ChildStderr);

#[cfg(test)]
mod test {
    use std::os::fd::{AsFd, AsRawFd};
    use std::process::Stdio;

    use crate::process::{Child, ChildStderr, ChildStdin, ChildStdout, Command};

    /// UT test cases for Child.
    ///
    /// # Brief
    /// 1. Create a ylong_runtime's `Child` with std `Child`.
    /// 2. Check stdin/stdout/stderr.
    /// 3. Call `wait()` to exit Child.
    #[test]
    fn ut_process_child_new_test() {
        let mut command = std::process::Command::new("echo");
        let std_child = command.spawn().unwrap();

        let handle = crate::spawn(async move {
            let mut child = Child::new(std_child, false, None, None, None).unwrap();
            assert!(!child.kill_on_drop);
            assert!(child.stdin.is_none());
            assert!(child.stdout.is_none());
            assert!(child.stderr.is_none());
            assert!(child.id().is_some());
            let status = child.wait().await.unwrap();
            assert!(status.success());
        });
        crate::block_on(handle).unwrap();
    }

    /// UT test cases for Child.
    ///
    /// # Brief
    /// 1. Create a `Command` with arg.
    /// 2. Use `spawn()` create a child handle
    /// 3. Use `try_wait()` waiting result until the child handle is ok.
    #[test]
    fn ut_process_try_wait_test() {
        let mut command = std::process::Command::new("echo");
        let std_child = command.spawn().unwrap();
        let handle = crate::spawn(async {
            let mut child = Child::new(std_child, false, None, None, None).unwrap();

            loop {
                if child.try_wait().unwrap().is_some() {
                    break;
                }
            }
            assert!(child.try_wait().unwrap().is_some());
        });
        crate::block_on(handle).unwrap();
    }

    /// UT test cases for stdio.
    ///
    /// # Brief
    /// 1. Create a `Command` with arg.
    /// 2. Use `spawn()` create a child handle
    /// 3. Use `wait()` waiting result.
    #[test]
    fn ut_process_stdio_test() {
        let handle = crate::spawn(async {
            let mut command = Command::new("echo");
            command
                .arg("Hello, world!")
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped());
            let mut child = command.spawn().unwrap();

            let child_stdin = child.take_stdin().unwrap();
            assert!(child_stdin.into_owned_fd().is_ok());
            let child_stdout = child.take_stdout().unwrap();
            assert!(child_stdout.into_owned_fd().is_ok());
            let child_stderr = child.take_stderr().unwrap();
            assert!(child_stderr.into_owned_fd().is_ok());

            drop(child);

            let mut child = command.spawn().unwrap();

            let child_stdin = child.take_stdin().unwrap();
            assert!(child_stdin.as_fd().as_raw_fd() >= 0);
            assert!(child_stdin.as_raw_fd() >= 0);
            assert!(TryInto::<Stdio>::try_into(child_stdin).is_ok());

            let child_stdout = child.take_stdout().unwrap();
            assert!(child_stdout.as_fd().as_raw_fd() >= 0);
            assert!(child_stdout.as_raw_fd() >= 0);
            assert!(TryInto::<Stdio>::try_into(child_stdout).is_ok());

            let child_stderr = child.take_stderr().unwrap();
            assert!(child_stderr.as_fd().as_raw_fd() >= 0);
            assert!(child_stderr.as_raw_fd() >= 0);
            assert!(TryInto::<Stdio>::try_into(child_stderr).is_ok());
            drop(child);
        });
        crate::block_on(handle).unwrap();
    }

    /// UT test cases for ChildStd.
    ///
    /// # Brief
    /// 1. Create a `std::process::Command`.
    /// 2. Use `spawn()` create a child handle
    /// 3. Use `from_std()` to convert std to ylong_runtime::process::ChildStd.
    #[test]
    fn ut_process_child_stdio_convert_test() {
        let mut command = std::process::Command::new("echo");
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = command.spawn().unwrap();
        let handle = crate::spawn(async move {
            let stdin = child.stdin.take().unwrap();
            assert!(ChildStdin::from_std(stdin).is_ok());
            let stdout = child.stdout.take().unwrap();
            assert!(ChildStdout::from_std(stdout).is_ok());
            let stderr = child.stderr.take().unwrap();
            assert!(ChildStderr::from_std(stderr).is_ok());
        });
        crate::block_on(handle).unwrap();
    }
}
