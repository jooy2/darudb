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
        use std::io::BufRead;
        use std::process::{Command, Stdio};

        let mut child = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", test, "--nocapture", "--test-threads", "1"])
            .env(HELPER_PATH, path)
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
