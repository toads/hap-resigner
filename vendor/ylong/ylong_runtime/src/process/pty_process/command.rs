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
use std::io;
use std::path::Path;
use std::process::{CommandArgs, CommandEnvs, Stdio};

use crate::process::pty_process::Pts;
use crate::process::{Child, Command};

/// A Command which spawn with Pty.
pub struct PtyCommand {
    command: Command,
    stdin: bool,
    stdout: bool,
    stderr: bool,
    f: Option<Box<dyn FnMut() -> io::Result<()> + Send + Sync + 'static>>,
}

impl PtyCommand {
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
    ///
    /// ```no_run
    /// use ylong_runtime::process::pty_process::PtyCommand;
    /// let _command = PtyCommand::new("sh");
    /// ```
    ///
    /// [#37519]: https://github.com/rust-lang/rust/issues/37519
    pub fn new<S: AsRef<OsStr>>(program: S) -> Self {
        Self {
            command: Command::new(program),
            stdin: false,
            stdout: false,
            stderr: false,
            f: None,
        }
    }

    /// Adds a parameter to pass to the program.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use ylong_runtime::process::pty_process::{Pty, PtyCommand};
    ///
    /// let pty = Pty::new().expect("Pty create fail!");
    /// let pts = pty.pts().expect("get pts fail!");
    ///
    /// PtyCommand::new("ls")
    ///     .arg("-l")
    ///     .arg("-a")
    ///     .spawn(&pts)
    ///     .expect("ls command failed to start");
    /// ```
    pub fn arg<S: AsRef<OsStr>>(&mut self, arg: S) -> &mut PtyCommand {
        self.command.arg(arg);
        self
    }

    /// Adds multiple parameters to pass to the program.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use ylong_runtime::process::pty_process::{Pty, PtyCommand};
    ///
    /// let pty = Pty::new().expect("Pty create fail!");
    /// let pts = pty.pts().expect("get pts fail!");
    ///
    /// PtyCommand::new("ls")
    ///     .args(["-l", "-a"])
    ///     .spawn(&pts)
    ///     .expect("ls command failed to start");
    /// ```
    pub fn args<I: IntoIterator<Item = S>, S: AsRef<OsStr>>(&mut self, args: I) -> &mut PtyCommand {
        self.command.args(args);
        self
    }

    /// Inserts or updates an environment variable mapping.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use ylong_runtime::process::pty_process::{Pty, PtyCommand};
    ///
    /// let pty = Pty::new().expect("Pty create fail!");
    /// let pts = pty.pts().expect("get pts fail!");
    ///
    /// PtyCommand::new("ls")
    ///     .env("PATH", "/bin")
    ///     .spawn(&pts)
    ///     .expect("ls command failed to start");
    /// ```
    pub fn env<K: AsRef<OsStr>, V: AsRef<OsStr>>(&mut self, key: K, val: V) -> &mut PtyCommand {
        self.command.env(key, val);
        self
    }

    /// Adds or updates multiple environment variable mappings.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use std::collections::HashMap;
    /// use std::env;
    /// use std::process::Stdio;
    ///
    /// use ylong_runtime::process::pty_process::{Pty, PtyCommand};
    ///
    /// let pty = Pty::new().expect("Pty create fail!");
    /// let pts = pty.pts().expect("get pts fail!");
    ///
    /// let filtered_env: HashMap<String, String> = env::vars()
    ///     .filter(|&(ref k, _)| k == "TERM" || k == "TZ" || k == "LANG" || k == "PATH")
    ///     .collect();
    ///
    /// PtyCommand::new("printenv")
    ///     .stdin(Stdio::null())
    ///     .stdout(Stdio::inherit())
    ///     .env_clear()
    ///     .envs(&filtered_env)
    ///     .spawn(&pts)
    ///     .expect("printenv failed to start");
    /// ```
    pub fn envs<I, K, V>(&mut self, vars: I) -> &mut PtyCommand
    where
        I: IntoIterator<Item = (K, V)>,
        K: AsRef<OsStr>,
        V: AsRef<OsStr>,
    {
        self.command.envs(vars);
        self
    }

    /// Removes an environment variable mapping.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use ylong_runtime::process::pty_process::{Pty, PtyCommand};
    ///
    /// let pty = Pty::new().expect("Pty create fail!");
    /// let pts = pty.pts().expect("get pts fail!");
    /// PtyCommand::new("ls")
    ///     .env_remove("PATH")
    ///     .spawn(&pts)
    ///     .expect("ls command failed to start");
    /// ```
    pub fn env_remove<K: AsRef<OsStr>>(&mut self, key: K) -> &mut PtyCommand {
        self.command.env_remove(key);
        self
    }

    /// Clears the entire environment map for the child process.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use ylong_runtime::process::pty_process::PtyCommand;
    ///
    /// PtyCommand::new("ls").env_clear();
    /// ```
    pub fn env_clear(&mut self) -> &mut PtyCommand {
        self.command.env_clear();
        self
    }

    /// Sets the working directory for the child process.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use ylong_runtime::process::pty_process::PtyCommand;
    ///
    /// PtyCommand::new("ls").current_dir("/bin");
    /// ```
    pub fn current_dir<P: AsRef<Path>>(&mut self, dir: P) -> &mut PtyCommand {
        self.command.current_dir(dir);
        self
    }

    /// Configuration for the child process's standard input (stdin) handle.
    /// Defaults to inherit when used with spawn or status, and defaults to
    /// piped when used with output.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use std::process::Stdio;
    ///
    /// use ylong_runtime::process::pty_process::PtyCommand;
    ///
    /// PtyCommand::new("ls").stdin(Stdio::null());
    /// ```
    pub fn stdin<T: Into<Stdio>>(&mut self, cfg: T) -> &mut PtyCommand {
        self.command.stdin(cfg);
        self.stdin = true;
        self
    }

    /// Configuration for the child process's standard output (stdout) handle.
    /// Defaults to inherit when used with spawn or status, and defaults to
    /// piped when used with output.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use std::process::Stdio;
    ///
    /// use ylong_runtime::process::pty_process::PtyCommand;
    ///
    /// PtyCommand::new("ls").stdout(Stdio::null());
    /// ```
    pub fn stdout<T: Into<Stdio>>(&mut self, cfg: T) -> &mut PtyCommand {
        self.command.stdout(cfg);
        self.stdout = true;
        self
    }

    /// Configuration for the child process's standard error (stderr) handle.
    /// Defaults to inherit when used with spawn or status, and defaults to
    /// piped when used with output.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use std::process::Stdio;
    ///
    /// use ylong_runtime::process::pty_process::PtyCommand;
    ///
    /// PtyCommand::new("ls").stderr(Stdio::null());
    /// ```
    pub fn stderr<T: Into<Stdio>>(&mut self, cfg: T) -> &mut PtyCommand {
        self.command.stderr(cfg);
        self.stderr = true;
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
    /// use ylong_runtime::process::pty_process::{Pty, PtyCommand};
    ///
    /// async fn command() -> std::process::ExitStatus {
    ///     let pty = Pty::new().expect("Pty create fail!");
    ///     let pts = pty.pts().expect("get pts fail!");
    ///     let mut child = PtyCommand::new("ls")
    ///         .spawn(&pts)
    ///         .expect("ls command failed to start");
    ///     child.wait().await.expect("ls command failed to run")
    /// }
    /// ```
    pub fn spawn(&mut self, pts: &Pts) -> io::Result<Child> {
        if !self.stdin {
            let stdin = pts.clone_stdio()?;
            self.command.stdin(stdin);
        }
        if !self.stdout {
            let stdout = pts.clone_stdio()?;
            self.command.stdout(stdout);
        }
        if !self.stderr {
            let stderr = pts.clone_stdio()?;
            self.command.stderr(stderr);
        }

        let mut session_leader = pts.session_leader();
        // session_leader do nothing unsafe.
        unsafe {
            if let Some(mut f) = self.f.take() {
                self.command.pre_exec(move || {
                    session_leader()?;
                    f()
                });
            } else {
                self.command.pre_exec(session_leader);
            }
        }

        self.command.spawn()
    }

    /// Returns the path to the program that was given to Command::new.
    ///
    /// # Example
    ///
    /// ```
    /// use ylong_runtime::process::pty_process::PtyCommand;
    ///
    /// let cmd = PtyCommand::new("echo");
    /// assert_eq!(cmd.get_program(), "echo");
    /// ```
    #[must_use]
    pub fn get_program(&self) -> &OsStr {
        self.command.get_program()
    }

    /// Returns an iterator of the arguments that will be passed to the program.
    ///
    /// This does not include the path to the program as the first argument;
    /// it only includes the arguments specified with Command::arg and
    /// Command::args.
    ///
    /// # Example
    ///
    /// ```
    /// use std::ffi::OsStr;
    ///
    /// use ylong_runtime::process::pty_process::PtyCommand;
    ///
    /// let mut cmd = PtyCommand::new("echo");
    /// cmd.arg("first").arg("second");
    /// let args: Vec<&OsStr> = cmd.get_args().collect();
    /// assert_eq!(args, &["first", "second"]);
    /// ```
    pub fn get_args(&self) -> CommandArgs<'_> {
        self.command.get_args()
    }

    /// Returns an iterator of the environment variables that will be set when
    /// the process is spawned.
    ///
    /// # Example
    ///
    /// ```
    /// use std::ffi::OsStr;
    ///
    /// use ylong_runtime::process::pty_process::PtyCommand;
    ///
    /// let mut cmd = PtyCommand::new("ls");
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
        self.command.get_envs()
    }

    /// Returns the working directory for the child process.
    ///
    /// This returns None if the working directory will not be changed.
    ///
    /// # Example
    ///
    /// ```
    /// use std::ffi::OsStr;
    /// use std::path::Path;
    ///
    /// use ylong_runtime::process::pty_process::PtyCommand;
    ///
    /// let mut cmd = PtyCommand::new("ls");
    /// assert_eq!(cmd.get_current_dir(), None);
    /// cmd.current_dir("/bin");
    /// assert_eq!(cmd.get_current_dir(), Some(Path::new("/bin")))
    /// ```
    #[must_use]
    pub fn get_current_dir(&self) -> Option<&Path> {
        self.command.get_current_dir()
    }

    /// Sets the child process's user ID. This translates to a `setuid` call in
    /// the child process. Failure in the `setuid` call will cause the spawn to
    /// fail.
    ///
    /// # Example
    ///
    /// ```
    /// use ylong_runtime::process::pty_process::PtyCommand;
    ///
    /// fn pty(id: u32) {
    ///     let mut cmd = PtyCommand::new("ls");
    ///     let gid = cmd.uid(id);
    /// }
    /// ```
    pub fn uid(&mut self, id: u32) -> &mut PtyCommand {
        self.command.uid(id);
        self
    }

    /// Similar to `uid`, but sets the group ID of the child process. This has
    /// the same semantics as the `uid` field.
    ///
    /// # Example
    ///
    /// ```
    /// use ylong_runtime::process::pty_process::PtyCommand;
    ///
    /// fn pty(id: u32) {
    ///     let mut cmd = PtyCommand::new("ls");
    ///     let gid = cmd.gid(id);
    /// }
    /// ```
    pub fn gid(&mut self, id: u32) -> &mut PtyCommand {
        self.command.gid(id);
        self
    }

    /// Set executable argument
    /// Set the first process argument `argv[0]`, to something other than the
    /// default executable path.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use ylong_runtime::process::pty_process::PtyCommand;
    ///
    /// let mut cmd = PtyCommand::new("ls");
    /// let gid = cmd.arg0("/path");
    /// ```
    pub fn arg0<S: AsRef<OsStr>>(&mut self, arg: S) -> &mut PtyCommand {
        self.command.arg0(arg);
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
    /// # Safety
    ///
    /// This closure will be run in the context of the child process after a
    /// `fork`. This primarily means that any modifications made to memory on
    /// behalf of this closure will ***not*** be visible to the parent process.
    /// This is often a very constrained environment where normal operations
    /// like `malloc`, accessing environment variables through [`mod@std::env`]
    /// or acquiring a mutex are not guaranteed to work (due to other
    /// threads perhaps still running when the `fork` was run).
    ///
    /// # Example
    ///
    /// ```no_run
    /// use ylong_runtime::process::pty_process::PtyCommand;
    ///
    /// let mut cmd = PtyCommand::new("ls");
    /// unsafe {
    ///     cmd.pre_exec(|| {
    ///         // do something
    ///         Ok(())
    ///     });
    /// }
    /// ```
    pub unsafe fn pre_exec<F>(&mut self, f: F) -> &mut PtyCommand
    where
        F: FnMut() -> io::Result<()> + Send + Sync + 'static,
    {
        self.f = Some(Box::new(f));
        self
    }

    /// Sets the process group ID (PGID) of the child process.
    /// Equivalent to a setpgid call in the child process, but may be more
    /// efficient. Process groups determine which processes receive signals.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use ylong_runtime::process::pty_process::PtyCommand;
    ///
    /// fn pty(pgid: i32) {
    ///     let mut cmd = PtyCommand::new("ls");
    ///     let gid = cmd.process_group(pgid);
    /// }
    /// ```
    pub fn process_group(&mut self, pgroup: i32) -> &mut PtyCommand {
        self.command.process_group(pgroup);
        self
    }
}

#[cfg(test)]
mod test {
    use std::ffi::OsStr;
    use std::path::Path;
    use std::process::Stdio;

    use crate::io::{AsyncReadExt, AsyncWriteExt};
    use crate::process::pty_process::{Pty, PtyCommand};

    /// UT test cases for PtyCommand.
    ///
    /// # Brief
    /// 1. Create a `PtyCommand`.
    /// 2. Set configs and check result is correct.
    #[test]
    fn ut_pty_process_basic_test() {
        let mut command = PtyCommand::new("echo");
        assert_eq!(command.get_program(), "echo");

        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        assert!(command.stdin);
        assert!(command.stdout);
        assert!(command.stderr);

        command.arg("first").args(["second"]);
        let args: Vec<&OsStr> = command.get_args().collect();
        assert_eq!(args, &["first", "second"]);

        command.env("PATH", "/bin");
        let envs: Vec<(&OsStr, Option<&OsStr>)> = command.get_envs().collect();
        assert_eq!(envs, &[(OsStr::new("PATH"), Some(OsStr::new("/bin")))]);

        command.env_remove("PATH");
        let envs: Vec<(&OsStr, Option<&OsStr>)> = command.get_envs().collect();
        assert_eq!(envs, &[(OsStr::new("PATH"), None)]);

        command.env_clear();
        let envs: Vec<(&OsStr, Option<&OsStr>)> = command.get_envs().collect();
        assert!(envs.is_empty());

        let envs = [(OsStr::new("TZ"), OsStr::new("test"))];
        command.envs(envs);
        let envs: Vec<(&OsStr, Option<&OsStr>)> = command.get_envs().collect();
        assert_eq!(envs, &[(OsStr::new("TZ"), Some(OsStr::new("test")))]);

        command.env_clear();
        let envs: Vec<(&OsStr, Option<&OsStr>)> = command.get_envs().collect();
        assert!(envs.is_empty());

        command.current_dir("/bin");
        assert_eq!(command.get_current_dir(), Some(Path::new("/bin")));
    }

    /// UT test cases for Pty read and write.
    ///
    /// # Brief
    /// 1. Create a `Pty` and a `Command`.
    /// 2. `spawn()` the child with pts of `Pty`.
    /// 3. Write `Pty` with arg.
    /// 4. Read `Pty` with correct result.
    #[test]
    fn ut_pty_process_read_write_test() {
        crate::block_on(async {
            let arg = "hello world!";
            let mut pty = Pty::new().unwrap();
            let pts = pty.pts().unwrap();

            let mut command = PtyCommand::new("echo");
            let mut child = command.spawn(&pts).unwrap();

            pty.write_all(arg.as_bytes()).await.unwrap();

            let status = child.wait().await.unwrap();
            assert!(status.success());

            let mut buf = [0; 14];
            pty.read_exact(&mut buf).await.unwrap();
            pty.flush().await.unwrap();
            pty.shutdown().await.unwrap();
            assert_eq!(String::from_utf8_lossy(&buf).replace(['\n', '\r'], ""), arg);
        });
    }

    /// UT test cases for pty split.
    ///
    /// # Brief
    /// 1. Create a `Pty` and a `Command` with arg.
    /// 2. `spawn()` the child with pts of `Pty`.
    /// 3. Write read_pty with arg.
    /// 4. Read write_pty with correct result.
    #[test]
    fn ut_pty_process_split_test() {
        crate::block_on(async {
            let arg = "hello world!";
            let mut pty = Pty::new().unwrap();
            let pts = pty.pts().unwrap();
            let (mut read_pty, mut write_pty) = pty.split();

            let mut command = PtyCommand::new("echo");
            let mut child = command.spawn(&pts).unwrap();

            write_pty.resize(24, 80, 0, 0).expect("resize set fail!");
            write_pty.write_all(arg.as_bytes()).await.unwrap();
            write_pty.flush().await.unwrap();
            write_pty.shutdown().await.unwrap();

            let status = child.wait().await.unwrap();
            assert!(status.success());

            let mut buf = [0; 14];
            read_pty.read_exact(&mut buf).await.unwrap();
            assert_eq!(String::from_utf8_lossy(&buf).replace(['\n', '\r'], ""), arg);
        });
    }

    /// UT test cases for pty into_split.
    ///
    /// # Brief
    /// 1. Create a `Pty` and a `Command` with arg.
    /// 2. `spawn()` the child with pts of `Pty`.
    /// 3. Write read_pty with arg.
    /// 4. Read write_pty with correct result.
    #[test]
    fn ut_pty_process_into_split_test() {
        crate::block_on(async {
            let arg = "hello world!";
            let pty = Pty::new().unwrap();
            let pts = pty.pts().unwrap();
            let (mut read_pty, mut write_pty) = pty.into_split();

            let mut command = PtyCommand::new("echo");
            let mut child = command.spawn(&pts).unwrap();

            write_pty.resize(24, 80, 0, 0).expect("resize set fail!");
            write_pty.write_all(arg.as_bytes()).await.unwrap();
            write_pty.flush().await.unwrap();
            write_pty.shutdown().await.unwrap();

            let status = child.wait().await.unwrap();
            assert!(status.success());

            let mut buf = [0; 14];
            read_pty.read_exact(&mut buf).await.unwrap();
            assert_eq!(String::from_utf8_lossy(&buf).replace(['\n', '\r'], ""), arg);
        });
    }

    /// UT test cases for pty unsplit.
    ///
    /// # Brief
    /// 1. Create a `Pty` and a `Command` with arg.
    /// 2. `unsplit()` read and write.
    /// 3. `spawn()` the child with pts of `Pty`.
    /// 4. Write pty with arg.
    /// 5. Read pty with correct result.
    #[test]
    fn ut_pty_process_unsplit_test() {
        crate::block_on(async {
            let arg = "hello world!";
            let pty = Pty::new().unwrap();
            let pts = pty.pts().unwrap();
            let (read_pty, write_pty) = pty.into_split();
            let mut pty = Pty::unsplit(read_pty, write_pty).expect("unsplit fail!");

            let mut command = PtyCommand::new("echo");
            let mut child = command.spawn(&pts).unwrap();

            pty.write_all(arg.as_bytes()).await.unwrap();

            let status = child.wait().await.unwrap();
            assert!(status.success());

            let mut buf = [0; 14];
            pty.read_exact(&mut buf).await.unwrap();
            assert_eq!(String::from_utf8_lossy(&buf).replace(['\n', '\r'], ""), arg);
        });
    }

    /// UT test cases for pty.
    ///
    /// # Brief
    /// 1. Create a `Pty` .
    /// 2. Parse pty to OwnedFd.
    /// 3. Check result is ok.
    #[test]
    fn ut_pty_as_test() {
        use std::os::fd::{AsFd, AsRawFd, OwnedFd};

        crate::block_on(async {
            let pty = Pty::new().unwrap();

            assert!(pty.as_fd().as_raw_fd() >= 0);
            assert!(pty.as_raw_fd() >= 0);
            let fd: OwnedFd = From::<Pty>::from(pty);
            assert!(fd.as_raw_fd() >= 0);
        });
    }

    /// UT test cases for pty debug.
    ///
    /// # Brief
    /// 1. Debug pty and splitPty.
    /// 2. Check format is correct.
    #[test]
    fn ut_pty_debug_test() {
        crate::block_on(async {
            let pty = Pty::new().unwrap();
            let pts = pty.pts().unwrap();
            assert!(format!("{pts:?}").contains("Pts(PtsInner(OwnedFd { fd:"));
            let (read_pty, write_pty) = pty.into_split();
            assert!(format!("{read_pty:?}")
                .contains("SplitReadPty(Pty(AsyncSource { io: Some(PtyInner(OwnedFd { fd:"));
            assert!(format!("{write_pty:?}")
                .contains("SplitWritePty(Pty(AsyncSource { io: Some(PtyInner(OwnedFd { fd:"));
            let mut pty = Pty::unsplit(read_pty, write_pty).expect("unsplit fail!");
            let (read_pty, write_pty) = pty.split();
            assert!(format!("{read_pty:?}")
                .contains("BorrowReadPty(Pty(AsyncSource { io: Some(PtyInner(OwnedFd { fd:"));
            assert!(format!("{write_pty:?}")
                .contains("BorrowWritePty(Pty(AsyncSource { io: Some(PtyInner(OwnedFd { fd:"));
        });
    }
}
