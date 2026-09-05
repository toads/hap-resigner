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

use std::ffi::OsStr;
use std::future::Future;
use std::io;
use std::path::Path;
use std::process::{Command as StdCommand, CommandArgs, CommandEnvs, ExitStatus, Output, Stdio};

use crate::process::child::{Child, ChildStderr, ChildStdin, ChildStdout};

/// Async version of std::process::Command
#[derive(Debug)]
pub struct Command {
    std: StdCommand,
    kill: bool,
}

/// # Example
///
/// ```
/// use std::process::Command;
/// let command = Command::new("echo");
/// let ylong_command = ylong_runtime::process::Command::new("hello");
/// ```
impl From<StdCommand> for Command {
    fn from(value: StdCommand) -> Self {
        Self {
            std: value,
            kill: false,
        }
    }
}

impl Command {
    /// Constructs a new Command for launching the program at path program, with
    /// the following default configuration:
    /// * No arguments to the program
    /// * Inherit the current process's environment
    /// * Inherit the current process's working directory
    /// * Inherit stdin/stdout/stderr for spawn or status, but create pipes for
    ///   output
    ///
    /// Builder methods are provided to change these defaults and otherwise
    /// configure the process. If program is not an absolute path, the PATH
    /// will be searched in an OS-defined way. The search path to be used
    /// may be controlled by setting the PATH environment variable on the
    /// Command, but this has some implementation limitations on Windows (see
    /// issue [#37519]).
    ///
    /// # Example
    /// ```
    /// use ylong_runtime::process::Command;
    /// let _command = Command::new("sh");
    /// ```
    ///
    /// [#37519]: https://github.com/rust-lang/rust/issues/37519
    pub fn new<S: AsRef<OsStr>>(program: S) -> Self {
        Self {
            std: StdCommand::new(program),
            kill: false,
        }
    }

    /// Gets std::process::Command from async `Command`
    ///
    /// # Example
    ///
    /// ```
    /// use ylong_runtime::process::Command;
    /// let command = Command::new("echo");
    /// let std_command = command.as_std();
    /// ```
    pub fn as_std(&self) -> &StdCommand {
        &self.std
    }

    /// Sets whether kill the child process when `Child` drop.
    /// The default value is false, it's similar to the behavior of the std.
    pub fn kill_on_drop(&mut self, kill: bool) -> &mut Command {
        self.kill = kill;
        self
    }

    /// Adds a parameter to pass to the program.
    ///
    /// It's same as std.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use ylong_runtime::process::Command;
    ///
    /// Command::new("ls")
    ///     .arg("-l")
    ///     .arg("-a")
    ///     .spawn()
    ///     .expect("ls command failed to start");
    /// ```
    pub fn arg<S: AsRef<OsStr>>(&mut self, arg: S) -> &mut Command {
        self.std.arg(arg);
        self
    }

    /// Adds multiple parameters to pass to the program.
    ///
    /// It's same as std.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use ylong_runtime::process::Command;
    ///
    /// Command::new("ls")
    ///     .args(["-l", "-a"])
    ///     .spawn()
    ///     .expect("ls command failed to start");
    /// ```
    pub fn args<I: IntoIterator<Item = S>, S: AsRef<OsStr>>(&mut self, args: I) -> &mut Command {
        self.std.args(args);
        self
    }

    /// Inserts or updates an environment variable mapping.
    ///
    /// It's same as std.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use ylong_runtime::process::Command;
    ///
    /// Command::new("ls")
    ///     .env("PATH", "/bin")
    ///     .spawn()
    ///     .expect("ls command failed to start");
    /// ```
    pub fn env<K: AsRef<OsStr>, V: AsRef<OsStr>>(&mut self, key: K, val: V) -> &mut Command {
        self.std.env(key, val);
        self
    }

    /// Adds or updates multiple environment variable mappings.
    ///
    /// It's same as std.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use std::collections::HashMap;
    /// use std::env;
    /// use std::process::Stdio;
    ///
    /// use ylong_runtime::process::Command;
    ///
    /// let filtered_env: HashMap<String, String> = env::vars()
    ///     .filter(|&(ref k, _)| k == "TERM" || k == "TZ" || k == "LANG" || k == "PATH")
    ///     .collect();
    ///
    /// Command::new("printenv")
    ///     .stdin(Stdio::null())
    ///     .stdout(Stdio::inherit())
    ///     .env_clear()
    ///     .envs(&filtered_env)
    ///     .spawn()
    ///     .expect("printenv failed to start");
    /// ```
    pub fn envs<I, S, V>(&mut self, vars: I) -> &mut Command
    where
        I: IntoIterator<Item = (S, V)>,
        S: AsRef<OsStr>,
        V: AsRef<OsStr>,
    {
        self.std.envs(vars);
        self
    }

    /// Removes an environment variable mapping.
    ///
    /// It's same as std.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use ylong_runtime::process::Command;
    ///
    /// Command::new("ls")
    ///     .env_remove("PATH")
    ///     .spawn()
    ///     .expect("ls command failed to start");
    /// ```
    pub fn env_remove<S: AsRef<OsStr>>(&mut self, key: S) -> &mut Command {
        self.std.env_remove(key);
        self
    }

    /// Clears the entire environment map for the child process.
    ///
    /// It's same as std.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use ylong_runtime::process::Command;
    ///
    /// Command::new("ls")
    ///     .env_clear()
    ///     .spawn()
    ///     .expect("ls command failed to start");
    /// ```
    pub fn env_clear(&mut self) -> &mut Command {
        self.std.env_clear();
        self
    }

    /// Sets the child process's working directory.
    ///
    /// It's same as std.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use ylong_runtime::process::Command;
    ///
    /// Command::new("ls")
    ///     .current_dir("/bin")
    ///     .spawn()
    ///     .expect("ls command failed to start");
    /// ```
    pub fn current_dir<P: AsRef<Path>>(&mut self, dir: P) -> &mut Command {
        self.std.current_dir(dir);
        self
    }

    /// Configuration for the child process's standard input (stdin) handle.
    /// Defaults to inherit when used with spawn or status, and defaults to
    /// piped when used with output.
    ///
    /// It's same as std.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use std::process::Stdio;
    ///
    /// use ylong_runtime::process::Command;
    ///
    /// Command::new("ls")
    ///     .stdin(Stdio::null())
    ///     .spawn()
    ///     .expect("ls command failed to start");
    /// ```
    pub fn stdin<T: Into<Stdio>>(&mut self, cfg: T) -> &mut Command {
        self.std.stdin(cfg);
        self
    }

    /// Configuration for the child process's standard output (stdout) handle.
    /// Defaults to inherit when used with spawn or status, and defaults to
    /// piped when used with output.
    ///
    /// It's same as std.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use std::process::Stdio;
    ///
    /// use ylong_runtime::process::Command;
    ///
    /// Command::new("ls")
    ///     .stdout(Stdio::null())
    ///     .spawn()
    ///     .expect("ls command failed to start");
    /// ```
    pub fn stdout<T: Into<Stdio>>(&mut self, cfg: T) -> &mut Command {
        self.std.stdout(cfg);
        self
    }

    /// Configuration for the child process's standard error (stderr) handle.
    /// Defaults to inherit when used with spawn or status, and defaults to
    /// piped when used with output.
    ///
    /// It's same as std.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use std::process::Stdio;
    ///
    /// use ylong_runtime::process::Command;
    ///
    /// Command::new("ls")
    ///     .stderr(Stdio::null())
    ///     .spawn()
    ///     .expect("ls command failed to start");
    /// ```
    pub fn stderr<T: Into<Stdio>>(&mut self, cfg: T) -> &mut Command {
        self.std.stderr(cfg);
        self
    }

    /// Executes the command as a child process, returning a handle to it.
    /// By default, stdin, stdout and stderr are inherited from the parent.
    ///
    /// This will spawn the child process synchronously and return a Future
    /// handle of child process.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use ylong_runtime::process::Command;
    ///
    /// async fn command() -> std::process::ExitStatus {
    ///     let mut child = Command::new("ls")
    ///         .spawn()
    ///         .expect("ls command failed to start");
    ///     child.wait().await.expect("ls command failed to run")
    /// }
    /// ```
    pub fn spawn(&mut self) -> io::Result<Child> {
        let mut child = self.std.spawn()?;
        let stdin = child
            .stdin
            .take()
            .map(super::sys::stdio)
            .transpose()?
            .map(ChildStdin::new);
        let stdout = child
            .stdout
            .take()
            .map(super::sys::stdio)
            .transpose()?
            .map(ChildStdout::new);
        let stderr = child
            .stderr
            .take()
            .map(super::sys::stdio)
            .transpose()?
            .map(ChildStderr::new);

        Child::new(child, self.kill, stdin, stdout, stderr)
    }

    /// Executes the command as a child process, waiting for it to finish and
    /// collecting all of its output. By default, stdout and stderr are
    /// captured (and used to provide the resulting output). Stdin is not
    /// inherited from the parent and any attempt by the child process to read
    /// from the stdin stream will result in the stream immediately closing.
    ///
    /// If set `kill_on_drop()`, the child will be killed when this method
    /// return.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use ylong_runtime::process::Command;
    ///
    /// async fn command() {
    ///     let output = Command::new("ls")
    ///         .output()
    ///         .await
    ///         .expect("ls command failed to run");
    ///     println!("stdout of ls: {:?}", output.stdout);
    /// }
    /// ```
    pub fn output(&mut self) -> impl Future<Output = io::Result<Output>> {
        self.stdout(Stdio::piped());
        self.stderr(Stdio::piped());

        let child = self.spawn();

        async { child?.output_wait().await }
    }

    /// Executes a command as a child process, waiting for it to finish and
    /// collecting its status. By default, stdin, stdout and stderr are
    /// inherited from the parent.
    ///
    /// If set `kill_on_drop()`, the child will be killed when this method
    /// return.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use ylong_runtime::process::Command;
    ///
    /// async fn command() -> std::process::ExitStatus {
    ///     Command::new("ls")
    ///         .status()
    ///         .await
    ///         .expect("Command status failed!")
    /// }
    /// ```
    /// This fn can only obtain `ExitStatus`. To obtain the `Output`, please use
    /// `output()`
    pub fn status(&mut self) -> impl Future<Output = io::Result<ExitStatus>> {
        let child = self.spawn();

        async {
            let mut child = child?;

            drop(child.take_stdin());
            drop(child.take_stdout());
            drop(child.take_stderr());

            child.wait().await
        }
    }

    /// Returns the path to the program that was given to Command::new.
    ///
    /// It's same as std.
    ///
    /// # Example
    ///
    /// ```
    /// use ylong_runtime::process::Command;
    ///
    /// let cmd = Command::new("echo");
    /// assert_eq!(cmd.get_program(), "echo");
    /// ```
    #[must_use]
    pub fn get_program(&self) -> &OsStr {
        self.std.get_program()
    }

    /// Returns an iterator of the arguments that will be passed to the program.
    ///
    /// This does not include the path to the program as the first argument;
    /// it only includes the arguments specified with Command::arg and
    /// Command::args.
    ///
    /// It's same as std.
    ///
    /// # Example
    ///
    /// ```
    /// use std::ffi::OsStr;
    ///
    /// use ylong_runtime::process::Command;
    ///
    /// let mut cmd = Command::new("echo");
    /// cmd.arg("first").arg("second");
    /// let args: Vec<&OsStr> = cmd.get_args().collect();
    /// assert_eq!(args, &["first", "second"]);
    /// ```
    pub fn get_args(&self) -> CommandArgs<'_> {
        self.std.get_args()
    }

    /// Returns an iterator of the environment variables that will be set when
    /// the process is spawned.
    ///
    /// It's same as std.
    ///
    /// # Example
    ///
    /// ```
    /// use std::ffi::OsStr;
    ///
    /// use ylong_runtime::process::Command;
    ///
    /// let mut cmd = Command::new("ls");
    /// cmd.env("TERM", "dumb").env_remove("TZ");
    /// let envs: Vec<(&OsStr, Option<&OsStr>)> = cmd.get_envs().collect();
    /// assert_eq!(
    ///     envs,
    ///     &[
    ///         (OsStr::new("TERM"), Some(OsStr::new("dumb"))),
    ///         (OsStr::new("TZ"), None)
    ///     ]
    /// );
    /// ```
    pub fn get_envs(&self) -> CommandEnvs<'_> {
        self.std.get_envs()
    }

    /// Returns the working directory for the child process.
    ///
    /// This returns None if the working directory will not be changed.
    ///
    /// It's same as std.
    ///
    /// # Example
    ///
    /// ```
    /// use std::ffi::OsStr;
    /// use std::path::Path;
    ///
    /// use ylong_runtime::process::Command;
    ///
    /// let mut cmd = Command::new("ls");
    /// assert_eq!(cmd.get_current_dir(), None);
    /// cmd.current_dir("/bin");
    /// assert_eq!(cmd.get_current_dir(), Some(Path::new("/bin")))
    /// ```
    #[must_use]
    pub fn get_current_dir(&self) -> Option<&Path> {
        self.std.get_current_dir()
    }
}

#[cfg(unix)]
use std::os::unix::process::CommandExt;

#[cfg(unix)]
impl Command {
    /// Sets the child process's user ID. This translates to a `setuid` call in
    /// the child process. Failure in the `setuid` call will cause the spawn to
    /// fail.
    ///
    /// It's same as std.
    pub fn uid(&mut self, id: u32) -> &mut Command {
        self.std.uid(id);
        self
    }

    /// Similar to `uid`, but sets the group ID of the child process. This has
    /// the same semantics as the `uid` field.
    ///
    /// It's same as std.
    pub fn gid(&mut self, id: u32) -> &mut Command {
        self.std.gid(id);
        self
    }

    /// Sets executable argument
    /// Sets the first process argument `argv[0]`, to something other than the
    /// default executable path.
    ///
    /// It's same as std.
    pub fn arg0<S: AsRef<OsStr>>(&mut self, arg: S) -> &mut Command {
        self.std.arg0(arg);
        self
    }

    /// Schedules a closure to be run just before the exec function is invoked.
    /// The closure is allowed to return an I/O error whose OS error code will
    /// be communicated back to the parent and returned as an error from when
    /// the spawn was requested. Multiple closures can be registered and
    /// they will be called in order of their registration. If a closure
    /// returns Err then no further closures will be called and the spawn
    /// operation will immediately return with a failure.
    ///
    /// It's same as std.
    ///
    /// # Safety
    ///
    /// This closure will be run in the context of the child process after a
    /// `fork`. This primarily means that any modifications made to memory on
    /// behalf of this closure will ***not*** be visible to the parent process.
    /// This is often a very constrained environment where normal operations
    /// like `malloc`, accessing environment variables through [`mod@std::env`]
    /// or acquiring a mutex are not guaranteed to work (due to other
    /// threads perhaps still running when the `fork` was run).
    pub unsafe fn pre_exec<F>(&mut self, f: F) -> &mut Command
    where
        F: FnMut() -> io::Result<()> + Send + Sync + 'static,
    {
        self.std.pre_exec(f);
        self
    }

    /// Sets the process group ID (PGID) of the child process.
    /// Equivalent to a setpgid call in the child process, but may be more
    /// efficient. Process groups determine which processes receive signals.
    ///
    /// It's same as std.
    pub fn process_group(&mut self, pgroup: i32) -> &mut Command {
        self.std.process_group(pgroup);
        self
    }
}

#[cfg(test)]
mod test {
    use std::io::IoSlice;
    use std::process::Stdio;

    use crate::io::{AsyncReadExt, AsyncWrite, AsyncWriteExt};
    use crate::process::Command;

    /// UT test cases for Command.
    ///
    /// # Brief
    /// 1. Create a `Command`.
    /// 2. Call `kill_on_drop()` set true and check command.kill.
    /// 3. Call `kill_on_drop()` set false and check command.kill.
    /// 4. Check command.std.
    #[test]
    fn ut_process_basic_test() {
        let mut command = Command::new("echo");
        assert!(!command.kill);
        command.kill_on_drop(true);
        assert!(command.kill);
        command.kill_on_drop(false);
        assert!(!command.kill);
        assert_eq!(command.std.get_program(), "echo");
    }

    /// UT test cases for `output()`.
    ///
    /// # Brief
    /// 1. Create a `Command` with arg.
    /// 2. Use `output()` waiting result.
    #[test]
    fn ut_process_output_test() {
        let handle = crate::spawn(async {
            let mut command = Command::new("echo");
            command.arg("Hello, world!");
            let output = command.output().await.unwrap();

            assert!(output.status.success());
            assert_eq!(output.stdout.as_slice(), b"Hello, world!\n");
            assert!(output.stderr.is_empty());
        });
        crate::block_on(handle).unwrap();
    }

    /// UT test cases for `status()`.
    ///
    /// # Brief
    /// 1. Create a `Command` with arg.
    /// 2. Use `status()` waiting result.
    #[test]
    fn ut_process_status_test() {
        let handle = crate::spawn(async {
            let mut command = Command::new("echo");
            command.arg("Hello, world!");

            let status = command.status().await.unwrap();
            assert!(status.success());
        });
        crate::block_on(handle).unwrap();
    }

    /// UT test cases for Command.
    ///
    /// # Brief
    /// 1. Create a `Command` and `spawn()`.
    /// 2. Take `child.stdin` and write something in it.
    /// 3. Take `child.stdout` and read it, check the result.
    /// 4. Check child's result.
    #[test]
    fn ut_process_child_stdio_test() {
        let handle = crate::spawn(async {
            let mut child = Command::new("rev")
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .expect("Failed to spawn child process");

            let mut stdin = child.take_stdin().expect("Failed to open stdin");
            let stdin_handle = crate::spawn(async move {
                assert!(stdin.is_write_vectored());
                stdin
                    .write_vectored(&[IoSlice::new(b"Hello, world!")])
                    .await
                    .unwrap();
                stdin.flush().await.unwrap();
                stdin.shutdown().await.unwrap();
            });

            let mut stdout = child.take_stdout().expect("Failed to open stdout");
            let stdout_handle = crate::spawn(async move {
                let mut buf = Vec::new();
                stdout.read_to_end(&mut buf).await.unwrap();
                let str = "!dlrow ,olleH";
                assert!(String::from_utf8(buf).unwrap().contains(str));
            });

            let mut stderr = child.take_stderr().expect("Failed to open stderr");
            let stderr_handle = crate::spawn(async move {
                let mut buf = Vec::new();
                stderr.read_to_end(&mut buf).await.unwrap();
                assert!(buf.is_empty());
            });

            let status = child.wait().await.unwrap();
            assert!(status.success());

            stdin_handle.await.unwrap();
            stdout_handle.await.unwrap();
            stderr_handle.await.unwrap();
        });
        crate::block_on(handle).unwrap();
    }

    /// Ut test cases for `kill()`.
    ///
    /// # Brief
    /// 1. Create a `Command` with arg.
    /// 2. Use `spawn()` create a child handle
    /// 3. Use `kill()` to kill the child handle.
    #[test]
    fn ut_process_kill_test() {
        let handle = crate::spawn(async {
            let mut command = Command::new("echo");
            command.arg("Hello, world!");
            let mut child = command.spawn().unwrap();

            assert!(child.kill().await.is_ok());
        });
        crate::block_on(handle).unwrap();
    }

    /// UT test cases for drop.
    ///
    /// # Brief
    /// 1. Create a `Command` with kill_on_drop.
    /// 2. Use `spawn()` create a child handle
    /// 3. Use `drop()` to drop the child handle.
    #[test]
    fn ut_process_drop_test() {
        let handle = crate::spawn(async {
            let mut command = Command::new("echo");
            command.arg("Hello, world!").kill_on_drop(true);
            let child = command.spawn();
            assert!(child.is_ok());
            drop(child.unwrap());

            let mut command = Command::new("echo");
            command.arg("Hello, world!");
            let child = command.spawn();
            assert!(child.is_ok());
            drop(child.unwrap());
        });
        crate::block_on(handle).unwrap();
    }

    /// UT test cases for command debug.
    ///
    /// # Brief
    /// 1. Debug Command and Child.
    /// 2. Check format is correct.
    #[test]
    fn ut_process_debug_test() {
        let handle = crate::spawn(async {
            let mut command = Command::new("echo");
            assert_eq!(
                format!("{command:?}"),
                "Command { std: \"echo\", kill: false }"
            );
            let mut child = command.spawn().unwrap();

            assert_eq!(format!("{child:?}"), "Child { state: Pending(Some(Child { stdin: None, stdout: None, stderr: None, .. })), kill_on_drop: false, stdin: None, stdout: None, stderr: None }");
            let status = child.wait().await.unwrap();
            assert!(status.success());
        });
        crate::block_on(handle).unwrap();
    }
}
