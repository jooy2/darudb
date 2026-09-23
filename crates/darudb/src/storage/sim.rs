//! A simulated disk for the crash tests.
//!
//! It keeps what the operating system holds and what is durable, as two
//! images. A barrier copies the first to the second. A simulated power cut
//! keeps the durable image plus any subset of the writes made since the last
//! barrier, in any order, each one whole, absent, or torn into a mix of old and
//! new bytes within its own range. A one-byte write is whole or absent, and
//! nothing outside a write's range ever changes. Those are exactly the
//! promises `design/README.md` says the engine relies on, and no more.

use std::io;
use std::sync::{Mutex, PoisonError};

use super::FileIo;
use crate::testing::Rng;

/// One write since the last barrier.
#[derive(Debug, Clone)]
enum Pending {
    Write { offset: usize, bytes: Vec<u8> },
    SetLen(usize),
}

#[derive(Debug, Default)]
struct State {
    durable: Vec<u8>,
    current: Vec<u8>,
    pending: Vec<Pending>,
}

/// A file that lives in memory and can lose power.
#[derive(Debug, Default)]
pub(crate) struct SimDisk {
    state: Mutex<State>,
}

impl SimDisk {
    /// A disk holding `image`, all of it durable.
    pub(crate) fn from_image(image: Vec<u8>) -> Self {
        Self {
            state: Mutex::new(State {
                durable: image.clone(),
                current: image,
                pending: Vec::new(),
            }),
        }
    }

    fn state(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// What a power cut right now could leave on the disk.
    pub(crate) fn power_cut(&self, rng: &mut Rng) -> Vec<u8> {
        let state = self.state();
        let mut image = state.durable.clone();
        let mut surviving: Vec<&Pending> =
            state.pending.iter().filter(|_| rng.below(3) != 0).collect();

        rng.shuffle(&mut surviving);

        for pending in surviving {
            match pending {
                Pending::SetLen(len) => image.resize(*len, 0),
                Pending::Write { offset, bytes } => {
                    if image.len() < offset + bytes.len() {
                        image.resize(offset + bytes.len(), 0);
                    }

                    let torn = bytes.len() > 1 && rng.below(2) == 0;

                    for (index, byte) in bytes.iter().enumerate() {
                        if !torn || rng.below(2) == 0 {
                            image[offset + index] = *byte;
                        }
                    }
                }
            }
        }

        image
    }
}

impl FileIo for SimDisk {
    fn read_at(&self, buf: &mut [u8], offset: u64) -> io::Result<()> {
        let state = self.state();
        let offset = usize::try_from(offset).map_err(io::Error::other)?;

        match state.current.get(offset..offset + buf.len()) {
            Some(bytes) => {
                buf.copy_from_slice(bytes);

                Ok(())
            }
            None => Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "the simulated file ended",
            )),
        }
    }

    fn write_at(&self, buf: &[u8], offset: u64) -> io::Result<()> {
        let mut state = self.state();
        let offset = usize::try_from(offset).map_err(io::Error::other)?;

        if state.current.len() < offset + buf.len() {
            state.current.resize(offset + buf.len(), 0);
        }

        state.current[offset..offset + buf.len()].copy_from_slice(buf);
        state.pending.push(Pending::Write {
            offset,
            bytes: buf.to_vec(),
        });

        Ok(())
    }

    fn sync(&self) -> io::Result<()> {
        let mut state = self.state();

        state.durable = state.current.clone();
        state.pending.clear();

        Ok(())
    }

    fn len(&self) -> io::Result<u64> {
        Ok(self.state().current.len() as u64)
    }

    fn set_len(&self, len: u64) -> io::Result<()> {
        let mut state = self.state();
        let len = usize::try_from(len).map_err(io::Error::other)?;

        state.current.resize(len, 0);
        state.pending.push(Pending::SetLen(len));

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_barrier_makes_writes_survive_a_power_cut() {
        let disk = SimDisk::default();
        let mut rng = Rng::new(1);

        disk.write_at(b"durable", 0).unwrap();
        disk.sync().unwrap();

        for _ in 0..20 {
            assert_eq!(disk.power_cut(&mut rng), b"durable");
        }
    }

    #[test]
    fn a_power_cut_keeps_old_or_new_bytes_and_nothing_else() {
        let disk = SimDisk::from_image(vec![b'o'; 16]);
        let mut rng = Rng::new(2);

        disk.write_at(&[b'n'; 8], 4).unwrap();

        let mut saw_torn = false;

        for _ in 0..200 {
            let image = disk.power_cut(&mut rng);

            assert_eq!(&image[..4], b"oooo");
            assert_eq!(&image[12..], b"oooo");
            assert!(
                image[4..12]
                    .iter()
                    .all(|byte| *byte == b'o' || *byte == b'n')
            );
            saw_torn |= image[4..12].contains(&b'o') && image[4..12].contains(&b'n');
        }

        assert!(saw_torn, "some cut tears the write");
    }

    #[test]
    fn a_one_byte_write_is_never_torn() {
        let disk = SimDisk::from_image(vec![0]);
        let mut rng = Rng::new(3);

        disk.write_at(&[0xFF], 0).unwrap();

        for _ in 0..50 {
            let byte = disk.power_cut(&mut rng)[0];

            assert!(byte == 0 || byte == 0xFF);
        }
    }
}
