//! Helpers shared by the engine's own tests.

/// A small deterministic random number generator (SplitMix64), so that a
/// failing randomized test can be replayed from its seed.
#[derive(Debug, Clone)]
pub(crate) struct Rng(u64);

impl Rng {
    pub(crate) fn new(seed: u64) -> Self {
        Self(seed)
    }

    pub(crate) fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);

        let mut z = self.0;

        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);

        z ^ (z >> 31)
    }

    /// A number from 0 up to, but not including, `bound`.
    pub(crate) fn below(&mut self, bound: u64) -> u64 {
        self.next_u64() % bound
    }

    /// An index into a collection of `len` items.
    pub(crate) fn index(&mut self, len: usize) -> usize {
        usize::try_from(self.below(len as u64)).unwrap()
    }

    /// `len` random bytes.
    pub(crate) fn bytes(&mut self, len: usize) -> Vec<u8> {
        (0..len).map(|_| self.next_u64().to_le_bytes()[0]).collect()
    }

    /// Puts `items` in a random order.
    pub(crate) fn shuffle<T>(&mut self, items: &mut [T]) {
        for index in (1..items.len()).rev() {
            let other = self.index(index + 1);

            items.swap(index, other);
        }
    }
}

/// Another process running one test of this test binary, killed when dropped.
///
/// Byte-range locks are only ever in conflict between processes on Unix-like
/// systems, so a test of the locks needs a second process. The test it runs
/// does its work only when it finds [`HELPER_PATH`] set, and returns at once
/// when the suite runs it as an ordinary test.
#[derive(Debug)]
pub(crate) struct Helper {
    child: std::process::Child,
    lines: std::sync::mpsc::Receiver<String>,
}

/// In a helper, waits for the test that started it to [`tell`](Helper::tell)
/// it to go on.
pub(crate) fn wait_to_be_told() {
    let mut line = String::new();

    std::io::stdin().read_line(&mut line).unwrap();
}

/// The variable that hands a helper its database's path.
pub(crate) const HELPER_PATH: &str = "DARUDB_HELPER_PATH";

impl Helper {
    /// Starts the test `test`, by its full path, in a new process, with
    /// [`HELPER_PATH`] set to `path`.
    pub(crate) fn spawn(test: &str, path: &std::path::Path) -> Self {
        Self::spawn_with(test, &[(HELPER_PATH, path.as_os_str())])
    }

    /// Starts the test `test`, by its full path, in a new process, with the
    /// environment variables `variables` set.
    pub(crate) fn spawn_with(test: &str, variables: &[(&str, &std::ffi::OsStr)]) -> Self {
        use std::io::BufRead;
        use std::process::{Command, Stdio};

        let mut child = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", test, "--nocapture", "--test-threads", "1"])
            .envs(variables.iter().copied())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let stdout = child.stdout.take().unwrap();
        let (sender, lines) = std::sync::mpsc::channel();

        std::thread::spawn(move || {
            for line in std::io::BufReader::new(stdout)
                .lines()
                .map_while(Result::ok)
            {
                if sender.send(line).is_err() {
                    break;
                }
            }
        });

        Self { child, lines }
    }

    /// Waits for the helper to print a line ending in `word`, and fails the
    /// test if it does not within a generous time. The test harness prints the
    /// test's name at the start of the first line of its output.
    pub(crate) fn wait_for(&self, word: &str) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);

        while let Some(left) = deadline.checked_duration_since(std::time::Instant::now()) {
            match self.lines.recv_timeout(left) {
                Ok(printed) if printed.split_whitespace().last() == Some(word) => return,
                Ok(_) => {}
                Err(_) => break,
            }
        }

        panic!("the helper never printed `{word}`");
    }

    /// Sends `command` to a helper that reads commands, and returns its answer:
    /// what follows `answer ` on the next line it prints that holds one.
    pub(crate) fn ask(&mut self, command: &str) -> String {
        use std::io::Write;

        let stdin = self.child.stdin.as_mut().unwrap();

        writeln!(stdin, "{command}").unwrap();
        stdin.flush().unwrap();

        self.answer()
    }

    /// The next answer the helper prints, unprompted or not.
    pub(crate) fn answer(&self) -> String {
        loop {
            let line = self
                .lines
                .recv_timeout(std::time::Duration::from_secs(60))
                .expect("the helper stopped answering");

            if let Some(at) = line.find("answer ") {
                return line[at + "answer ".len()..].to_owned();
            }
        }
    }

    /// Lets a helper waiting in [`wait_to_be_told`] go on.
    pub(crate) fn tell(&mut self) {
        use std::io::Write;

        let stdin = self.child.stdin.as_mut().unwrap();

        stdin.write_all(b"go on\n").unwrap();
        stdin.flush().unwrap();
    }

    /// The lines the helper has printed since the last call, without waiting.
    pub(crate) fn printed(&self) -> Vec<String> {
        self.lines.try_iter().collect()
    }

    /// Kills the helper, waits until it is gone with every lock it held, and
    /// returns what it printed that was not read yet.
    pub(crate) fn kill_and_read(mut self) -> Vec<String> {
        self.stop();

        // Its output ends once it is gone.
        self.lines.iter().collect()
    }

    /// Closes the helper's input, which tells a helper that reads it to stop.
    pub(crate) fn close_input(&mut self) {
        drop(self.child.stdin.take());
    }

    /// Closes the helper's input if it is still open, and waits for the
    /// helper. Returns whether it exited normally, and what it printed that
    /// was not read yet.
    pub(crate) fn finish(mut self) -> (bool, Vec<String>) {
        self.close_input();

        let exited = self.child.wait().is_ok_and(|status| status.success());

        (exited, self.lines.iter().collect())
    }

    /// Kills the helper and waits until it is gone, with every lock it held.
    pub(crate) fn kill(mut self) {
        self.stop();
    }

    fn stop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Drop for Helper {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Set, a pause for the next reader that registers a snapshot afresh: it
/// says so on the first channel after its first read of the header, and waits
/// for the second before it takes the snapshot's lock. The one test of the
/// second read sets it, for the file it names; it catches one reader of that
/// file and is gone. Readers of other files, which other tests run in the
/// same process at the same time, pass by.
pub(crate) static PAUSE_BEFORE_REGISTERING: std::sync::Mutex<Option<Pause>> =
    std::sync::Mutex::new(None);

/// The file a reader stops for, and the channels it says so and waits on.
pub(crate) type Pause = (
    std::path::PathBuf,
    std::sync::mpsc::Sender<()>,
    std::sync::mpsc::Receiver<()>,
);

/// Where a reader of the file at `path` stops if [`PAUSE_BEFORE_REGISTERING`]
/// names that file. Only test builds call it.
pub(crate) fn pause_before_registering(path: &std::path::Path) {
    pause_at(&PAUSE_BEFORE_REGISTERING, path);
}

/// Set, a pause for the next salvage of the file it names, once the salvage
/// holds the file: it says so on the first channel, and waits for the
/// second. The test of opening a file under salvage sets it.
pub(crate) static PAUSE_IN_SALVAGE: std::sync::Mutex<Option<Pause>> = std::sync::Mutex::new(None);

/// Where a salvage of the file at `path` stops if [`PAUSE_IN_SALVAGE`] names
/// that file. Only test builds call it.
pub(crate) fn pause_in_salvage(path: &std::path::Path) {
    pause_at(&PAUSE_IN_SALVAGE, path);
}

/// Stops at `slot`'s pause if it names the file at `path`, taking it.
fn pause_at(slot: &std::sync::Mutex<Option<Pause>>, path: &std::path::Path) {
    let pause = {
        let mut pause = slot
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);

        match &*pause {
            Some((file, _, _)) if file == path => pause.take(),
            _ => None,
        }
    };

    if let Some((_, paused, resume)) = pause {
        let _ = paused.send(());
        let _ = resume.recv();
    }
}

/// Whether [`pause_in_recovery`] pauses. A helper sets it before it opens a
/// database.
pub(crate) static PAUSE_IN_RECOVERY: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// Stops a process for good in the middle of opening a file it recovers, if
/// [`PAUSE_IN_RECOVERY`] is set, after saying so: the test kills it there, to
/// show that the next process to open the file recovers it. Only test builds
/// call it.
pub(crate) fn pause_in_recovery() {
    if PAUSE_IN_RECOVERY.load(std::sync::atomic::Ordering::Relaxed) {
        println!("\nanswer recovering");

        loop {
            std::thread::sleep(std::time::Duration::from_secs(1));
        }
    }
}

/// Whether snapshot locks are kept for a moment after their last reader, as
/// they are outside the tests. The multi-process suite turns it off in half
/// its workers, whose readers would otherwise join kept locks and seldom
/// register a snapshot afresh.
pub(crate) static KEEP_SNAPSHOT_LOCKS: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(true);
