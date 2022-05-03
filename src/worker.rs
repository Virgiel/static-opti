use std::{
    io::{BufWriter, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicUsize, Ordering},
};

use brotli::CompressorWriter;
use libdeflater::{CompressionLvl, Compressor};
use tempfile::NamedTempFile;

/// Concurrent queue
struct StaticQueue<T> {
    items: Vec<T>,
    pos: AtomicUsize,
}

impl<T> StaticQueue<T> {
    /// Create new queue
    pub fn new(items: Vec<T>) -> Self {
        Self {
            pos: AtomicUsize::new(items.len()),
            items,
        }
    }

    /// Pop item from queue
    pub fn pop(&self) -> Option<&T> {
        let pos = self
            .pos
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |v| {
                Some(v.saturating_sub(1))
            })
            .unwrap();
        if pos > 0 {
            Some(&self.items[pos - 1])
        } else {
            None
        }
    }
}

/// Optimized item
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct Item {
    pub path: String,
    pub plain: (String, (u64, u32)),
    pub gzip: Option<(String, (u64, u32))>,
    pub brotli: Option<(String, (u64, u32))>,
}

type CompressedFile = (String, Vec<u8>, Option<Vec<u8>>, Option<Vec<u8>>);

/// Optimizer accumulator with a tempfile buffer
pub struct Accumulator {
    writer: BufWriter<NamedTempFile>,
    items: Vec<Item>,
    count: u64,
}

impl Accumulator {
    /// New empty accumulator
    pub fn new() -> Self {
        Self {
            writer: BufWriter::new(NamedTempFile::new().unwrap()),
            items: vec![],
            count: 0,
        }
    }

    fn append(&mut self, content: &[u8]) -> (u64, u32) {
        let start = self.count;
        self.count += content.len() as u64;
        self.writer.write_all(content).unwrap();
        (start, content.len() as u32)
    }

    /// Add a new compressed file
    pub fn add(&mut self, file: CompressedFile) {
        let (path, content, gzip, brotli) = file;
        let item = Item {
            path,
            plain: (etag(&content), self.append(&content)),
            gzip: gzip.map(|content| (etag(&content), self.append(&content))),
            brotli: brotli.map(|content| (etag(&content), self.append(&content))),
        };
        self.items.push(item);
    }

    /// Merge two accumulator
    pub fn merge(mut self, other: Self) -> Self {
        // Copy items with new pos;
        self.items.extend(other.items.into_iter().map(|mut item| {
            item.plain.1 .0 += self.count;
            item.gzip.iter_mut().for_each(|it| it.1 .0 += self.count);
            item.brotli.iter_mut().for_each(|it| it.1 .0 += self.count);
            return item;
        }));
        // Copy other file from start
        let mut file = other.writer.into_inner().unwrap();
        file.seek(SeekFrom::Start(0)).unwrap();
        std::io::copy(&mut file, &mut self.writer).unwrap();
        // Increment count
        self.count += other.count;
        return self;
    }

    /// Persist accumulator buffer in a file, return optimized items
    pub fn persist(self, path: &Path) -> Vec<Item> {
        let file = self.writer.into_inner().unwrap();
        file.persist(path).unwrap();
        self.items
    }
}

/// Compress a file;
fn compress_file(file: &Path, parent: &Path) -> CompressedFile {
    // Read plain file
    let plain = std::fs::read(&file).unwrap();
    // Format path
    let path = file
        .strip_prefix(parent)
        .unwrap()
        .to_str()
        .unwrap()
        .replace("\\", "/"); // Normalized path separator

    if plain.is_empty() {
        (path, plain, None, None)
    } else {
        // Gzip compress
        let mut compressor = Compressor::new(CompressionLvl::best());
        let max_size = compressor.gzip_compress_bound(plain.len());
        let mut gzip = vec![0; max_size];
        let gzip_size = compressor.gzip_compress(&plain, &mut gzip).unwrap();
        gzip.resize(gzip_size, 0);
        let gzip = (gzip.len() * 100 / plain.len() < 90).then(|| gzip);

        // Brotli compress
        let mut brotli = Vec::new();
        let mut writer = CompressorWriter::new(&mut brotli, 4096, 11, 24);
        writer.write_all(&plain).unwrap();
        writer.flush().unwrap();
        writer.into_inner();
        let brotli = (brotli.len() * 100 / plain.len() < 90).then(|| brotli);

        (path, plain, gzip, brotli)
    }
}

/// Compress a whole directory and return the resulting accumulator
pub fn compress_dir(dir: impl AsRef<Path>) -> Accumulator {
    let in_dir = dir.as_ref();
    let mut entries = Vec::new();
    walk(in_dir, &mut entries);
    let queue = StaticQueue::new(entries);
    // Parallel compression
    crossbeam::thread::scope(|s| {
        let accs: Vec<_> = (0..std::thread::available_parallelism().unwrap().get())
            .into_iter()
            .map(|_| {
                let queue = &queue;
                s.spawn(move |_| {
                    let mut acc = Accumulator::new();
                    while let Some(path) = queue.pop() {
                        acc.add(compress_file(&path, &in_dir))
                    }
                    return acc;
                })
            })
            .collect();
        // Merge
        accs.into_iter()
            .map(|it| it.join().unwrap())
            .reduce(|a, b| a.merge(b))
            .unwrap_or_else(|| Accumulator::new())
    })
    .unwrap()
}

/// Generate strong etag from bytes
fn etag(bytes: &[u8]) -> String {
    let hash = xxhash_rust::xxh3::xxh3_128(bytes);
    base64::encode_config(hash.to_le_bytes(), base64::URL_SAFE_NO_PAD)
}

/// Recursive walk of any file in a directory
fn walk(path: &Path, paths: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(path).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        if entry.file_type().unwrap().is_file() {
            paths.push(path);
        } else {
            walk(&path, paths);
        }
    }
}

/// Optimize a directory into another, returning optimized items
pub fn optimize(in_dir: &Path, out_dir: &Path) -> Vec<Item> {
    std::fs::remove_dir_all(&out_dir).ok();
    std::fs::create_dir_all(&out_dir).unwrap();
    let out_file = out_dir.join("out.static");
    let acc = compress_dir(in_dir);
    let mut items = acc.persist(&out_file);
    items.sort_unstable_by(|a, b| a.path.cmp(&b.path));
    // Write report
    std::fs::write(
        out_dir.join("report.json"),
        &serde_json::to_vec(&items).unwrap(),
    )
    .unwrap();
    return items;
}
