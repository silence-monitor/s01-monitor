//! Where a log goes while it is being written: a file on a computer, or in a
//! browser, where there are no files, the browser's own storage in chunks (see
//! `ChunkStore`) or, if that is not there, memory. A browser log is handed to
//! the person as a download when it stops.

#![allow(dead_code)]

use std::fs::File;
use std::io::{self, BufWriter, Write};
use std::path::Path;

use web_time::{Duration, Instant};

/// A browser's storage for the pieces of a recording, as it is made, so a long
/// recording does not sit in memory and survives the page being closed. The
/// real one is `platform.rs` talking to IndexedDB; the tests use a fake.
pub trait ChunkStore: std::fmt::Debug + Send {
    /// A recording has begun, to be saved as `name`.
    fn begin(&mut self, id: &str, name: &str);
    /// The next piece, numbered from 0.
    fn append(&mut self, id: &str, seq: u32, text: &str);
    /// A problem with the storage, in words, once there is one. The store
    /// carries on in memory, so the recording is not lost.
    fn status(&self, id: &str) -> Option<String>;
}

/// A piece goes to the store when it reaches this size, or when this long has
/// passed since the last one.
const CHUNK_BYTES: usize = 64 * 1024;
const CHUNK_EVERY: Duration = Duration::from_secs(5);

/// A recording kept in a `ChunkStore`: what is written waits in `buffer` until
/// there is enough, or it has waited long enough, to go to the store.
#[derive(Debug)]
pub struct Chunked {
    store: Box<dyn ChunkStore>,
    id: String,
    buffer: Vec<u8>,
    seq: u32,
    last_push: Instant,
    bytes: usize,
    every: Duration,
}

impl Chunked {
    fn push_if_due(&mut self) {
        if self.buffer.is_empty() {
            return;
        }
        if self.buffer.len() >= self.bytes || self.last_push.elapsed() >= self.every {
            let text = String::from_utf8_lossy(&self.buffer).into_owned();
            self.store.append(&self.id, self.seq, &text);
            self.buffer.clear();
            self.seq += 1;
            self.last_push = Instant::now();
        }
    }
}

#[derive(Debug)]
pub enum Sink {
    File(BufWriter<File>),
    Memory(Vec<u8>),
    Chunked(Chunked),
}

impl Sink {
    pub fn file(path: &Path) -> io::Result<Self> {
        Ok(Self::File(BufWriter::new(File::create(path)?)))
    }

    pub fn memory() -> Self {
        Self::Memory(Vec::new())
    }

    /// A recording kept piece by piece in a browser's storage.
    pub fn chunked(mut store: Box<dyn ChunkStore>, id: &str, name: &str) -> Self {
        store.begin(id, name);
        Self::Chunked(Chunked {
            store,
            id: id.to_string(),
            buffer: Vec::new(),
            seq: 0,
            last_push: Instant::now(),
            bytes: CHUNK_BYTES,
            every: CHUNK_EVERY,
        })
    }

    /// Like `chunked`, with the size and time limits given, for tests.
    #[cfg(test)]
    pub fn chunked_with(store: Box<dyn ChunkStore>, id: &str, name: &str, bytes: usize, every: Duration) -> Self {
        let mut sink = Self::chunked(store, id, name);
        if let Self::Chunked(chunked) = &mut sink {
            chunked.bytes = bytes;
            chunked.every = every;
        }
        sink
    }

    /// What has not gone to the store yet, with the recording's id. It goes to
    /// the store with the final download. Only a chunked sink has any.
    pub fn take_tail(&mut self) -> Option<(String, Vec<u8>)> {
        match self {
            Self::Chunked(chunked) => Some((chunked.id.clone(), std::mem::take(&mut chunked.buffer))),
            _ => None,
        }
    }

    /// A problem the store has reported, if this is a chunked sink.
    pub fn store_warning(&self) -> Option<String> {
        match self {
            Self::Chunked(chunked) => chunked.store.status(&chunked.id),
            _ => None,
        }
    }

    pub fn is_memory(&self) -> bool {
        matches!(self, Self::Memory(_))
    }

    /// The bytes written so far, for a memory sink. A file sink has none.
    pub fn take_memory(&mut self) -> Option<Vec<u8>> {
        match self {
            Self::Memory(bytes) => Some(std::mem::take(bytes)),
            Self::File(_) | Self::Chunked(_) => None,
        }
    }
}

impl Write for Sink {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match self {
            Self::File(out) => out.write(buf),
            Self::Memory(bytes) => bytes.write(buf),
            Self::Chunked(chunked) => {
                chunked.buffer.extend_from_slice(buf);
                Ok(buf.len())
            }
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        match self {
            Self::File(out) => out.flush(),
            Self::Memory(_) => Ok(()),
            Self::Chunked(chunked) => {
                chunked.push_if_due();
                Ok(())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_memory_sink_keeps_what_was_written_and_gives_it_up_once() {
        let mut sink = Sink::memory();
        write!(sink, "one ").unwrap();
        write!(sink, "two").unwrap();
        sink.flush().unwrap();
        assert!(sink.is_memory());
        assert_eq!(sink.take_memory().unwrap(), b"one two");
        assert_eq!(sink.take_memory().unwrap(), Vec::<u8>::new());
    }

    #[test]
    fn a_file_sink_has_no_memory_to_give() {
        let path = std::env::temp_dir().join("s01-sink-test.txt");
        let mut sink = Sink::file(&path).unwrap();
        write!(sink, "x").unwrap();
        sink.flush().unwrap();
        assert!(!sink.is_memory());
        assert!(sink.take_memory().is_none());
        assert_eq!(std::fs::read(&path).unwrap(), b"x");
        let _ = std::fs::remove_file(path);
    }
    use std::sync::{Arc, Mutex};

    /// A store that keeps what it is given where the test can see it.
    #[derive(Debug, Default, Clone)]
    struct FakeStore {
        began: Arc<Mutex<Vec<(String, String)>>>,
        chunks: Arc<Mutex<Vec<(String, u32, String)>>>,
        problem: Arc<Mutex<Option<String>>>,
    }

    impl ChunkStore for FakeStore {
        fn begin(&mut self, id: &str, name: &str) {
            self.began.lock().unwrap().push((id.into(), name.into()));
        }
        fn append(&mut self, id: &str, seq: u32, text: &str) {
            self.chunks.lock().unwrap().push((id.into(), seq, text.into()));
        }
        fn status(&self, _id: &str) -> Option<String> {
            self.problem.lock().unwrap().clone()
        }
    }

    #[test]
    fn a_chunked_sink_tells_the_store_it_has_begun_and_holds_small_writes_back() {
        let store = FakeStore::default();
        let mut sink = Sink::chunked_with(Box::new(store.clone()), "rec-1", "recording-1.log", 100, Duration::from_secs(3600));
        assert_eq!(store.began.lock().unwrap().as_slice(), [("rec-1".to_string(), "recording-1.log".to_string())]);
        write!(sink, "short").unwrap();
        sink.flush().unwrap();
        assert!(store.chunks.lock().unwrap().is_empty(), "too small and too soon to go yet");
        assert!(!sink.is_memory());
        assert!(sink.take_memory().is_none());
    }

    #[test]
    fn a_chunked_sink_sends_a_piece_when_it_is_big_enough_numbered_from_zero() {
        let store = FakeStore::default();
        let mut sink = Sink::chunked_with(Box::new(store.clone()), "rec-2", "r.log", 10, Duration::from_secs(3600));
        for part in ["aaaaaa\n", "bbbbbb\n", "cc\n", "dddddddddd\n"] {
            write!(sink, "{part}").unwrap();
            sink.flush().unwrap();
        }
        let chunks = store.chunks.lock().unwrap().clone();
        let seqs: Vec<u32> = chunks.iter().map(|(_, seq, _)| *seq).collect();
        assert_eq!(seqs, [0, 1], "pieces are numbered from 0 in order: {chunks:?}");
        assert!(chunks.iter().all(|(id, _, _)| id == "rec-2"));
        assert_eq!(chunks[0].2, "aaaaaa\nbbbbbb\n", "the first piece went once it passed 10 bytes");
        assert_eq!(chunks[1].2, "cc\ndddddddddd\n");
    }

    #[test]
    fn a_chunked_sink_also_sends_what_it_holds_when_enough_time_has_passed() {
        let store = FakeStore::default();
        let mut sink = Sink::chunked_with(Box::new(store.clone()), "rec-3", "r.log", 1_000_000, Duration::ZERO);
        write!(sink, "one\n").unwrap();
        sink.flush().unwrap();
        write!(sink, "two\n").unwrap();
        sink.flush().unwrap();
        let texts: Vec<String> = store.chunks.lock().unwrap().iter().map(|(_, _, text)| text.clone()).collect();
        assert_eq!(texts, ["one\n", "two\n"]);
        // Nothing waiting, so a flush sends nothing more.
        sink.flush().unwrap();
        assert_eq!(store.chunks.lock().unwrap().len(), 2);
    }

    #[test]
    fn the_tail_is_what_the_store_has_not_got_and_the_pieces_plus_the_tail_are_everything() {
        let store = FakeStore::default();
        let mut sink = Sink::chunked_with(Box::new(store.clone()), "rec-4", "r.log", 8, Duration::from_secs(3600));
        let written = "0123456789\nabcdef\nxyz\nlast";
        for line in written.split_inclusive('\n') {
            write!(sink, "{line}").unwrap();
            sink.flush().unwrap();
        }
        let (id, tail) = sink.take_tail().unwrap();
        assert_eq!(id, "rec-4");
        let mut all: String = store.chunks.lock().unwrap().iter().map(|(_, _, text)| text.as_str()).collect();
        all.push_str(&String::from_utf8(tail).unwrap());
        assert_eq!(all, written, "nothing lost and nothing twice");
        // Taken once.
        assert_eq!(sink.take_tail().unwrap().1, Vec::<u8>::new());
        // A file sink has none.
        let path = std::env::temp_dir().join("s01-sink-tail-test.txt");
        let mut file = Sink::file(&path).unwrap();
        assert!(file.take_tail().is_none());
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn a_problem_the_store_reports_reaches_the_sink() {
        let store = FakeStore::default();
        let sink = Sink::chunked_with(Box::new(store.clone()), "rec-5", "r.log", 10, Duration::from_secs(1));
        assert!(sink.store_warning().is_none());
        *store.problem.lock().unwrap() = Some("quota".into());
        assert_eq!(sink.store_warning().as_deref(), Some("quota"));
        assert!(Sink::memory().store_warning().is_none());
    }
}
