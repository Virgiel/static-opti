use std::{
    io::{BufWriter, Write},
    path::{Path, PathBuf},
};

use brotli::CompressorWriter;
use hashbrown::HashMap;
use libdeflater::{CompressionLvl, Compressor};
use memmap2::Mmap;
use rayon::iter::{IntoParallelRefIterator, ParallelIterator};

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct Item {
    pub path: String,
    pub plain: (String, (u64, u32)),
    pub gzip: Option<(String, (u64, u32))>,
    pub brotli: Option<(String, (u64, u32))>,
}

pub struct Match<'a> {
    pub path: &'a str,
    pub content: &'a [u8],
    pub etag: &'a str,
    pub encoding: Option<&'a str>,
}

pub struct FilesService {
    mmap: Mmap,
    map: HashMap<String, Item>,
}

impl FilesService {
    /// Create a file service from an already optimized dir
    pub fn new(static_dir: impl AsRef<Path>) -> Self {
        let dir = static_dir.as_ref();
        let bytes = std::fs::read(dir.join("report.json")).unwrap();
        let items: Vec<Item> = serde_json::from_slice(&bytes).unwrap();
        Self::from_item_and_path(items, &dir.join("out.static"))
    }

    /// Create and optimized file service at runtime
    pub fn build(static_dir: impl AsRef<Path>, temp_file: impl AsRef<Path>) -> Self {
        let out = temp_file.as_ref();
        let items = compress_merge(static_dir, out);
        Self::from_item_and_path(items, out)
    }

    fn from_item_and_path(items: Vec<Item>, path: &Path) -> Self {
        let mmap = unsafe { Mmap::map(&std::fs::File::open(path).unwrap()).unwrap() };
        let mut map = HashMap::new();
        for item in items {
            map.insert(item.path.clone(), item);
        }

        Self { mmap, map }
    }

    /// Find a matching file
    pub fn find(&self, accept_encoding: &str, path: &str) -> Option<Match> {
        let path = path.trim_matches('/');
        if let Some(it) = self.map.get(path) {
            return Some(self.match_item(accept_encoding, it));
        }

        {
            let path = if path == "" {
                "index.html".to_string()
            } else {
                format!("{}/index.html", path)
            };

            if let Some(it) = self.map.get(&path) {
                return Some(self.match_item(accept_encoding, it));
            }
        }

        let path = format!("{}.html", path);

        if let Some(it) = self.map.get(&path) {
            return Some(self.match_item(accept_encoding, it));
        }

        return None;
    }

    /// Construct match from an item and an accept encoding header value
    fn match_item<'a>(&'a self, accept_encoding: &str, item: &'a Item) -> Match {
        let (encoding, (etag, (start, len))) = match_encoding_tag(accept_encoding, item);
        Match {
            path: &item.path,
            content: &self.mmap[*start as usize..][..*len as usize],
            etag,
            encoding,
        }
    }
}

/// Extract supported encoding and corresponding tag
fn match_encoding_tag<'a>(
    accept_encoding: &str,
    item: &'a Item,
) -> (Option<&'static str>, &'a (String, (u64, u32))) {
    if let Some(it) = &item.brotli {
        if accept_encoding.contains("br") {
            return (Some("br"), it);
        }
    }
    if let Some(it) = &item.gzip {
        if accept_encoding.contains("gzip") {
            return (Some("gzip"), it);
        }
    }
    (None, &item.plain)
}

pub fn compress_merge(in_dir: impl AsRef<Path>, out_file: impl AsRef<Path>) -> Vec<Item> {
    let mut entries = Vec::new();

    let in_dir = in_dir.as_ref();
    walk(in_dir, &mut entries);
    // Parallel compression
    let compressed: Vec<_> = entries
        .par_iter()
        .map(|p| {
            // Read plain file
            let plain = std::fs::read(&p).unwrap();
            // Format path
            let path = p
                .strip_prefix(in_dir)
                .unwrap()
                .to_str()
                .unwrap()
                .to_string()
                .replace("\\", "/");
            let path = path.trim_matches('/').to_string();
            // Gzip compress
            let mut compressor = Compressor::new(CompressionLvl::best());
            let max_size = compressor.gzip_compress_bound(plain.len());
            let mut gzip = vec![0; max_size];
            let gzip_size = compressor.gzip_compress(&plain, &mut gzip).unwrap();
            gzip.resize(gzip_size, 0);
            let gzip = (gzip.len() * 100 / plain.len() < 90).then(|| (etag(&gzip), gzip));

            // Brotli compress
            let mut brotli = Vec::new();
            let mut writer = CompressorWriter::new(&mut brotli, 4096, 11, 24);
            writer.write_all(&plain).unwrap();
            writer.flush().unwrap();
            writer.into_inner();
            let brotli = (brotli.len() * 100 / plain.len() < 90).then(|| (etag(&brotli), brotli));

            (path, (etag(&plain), plain), gzip, brotli)
        })
        .collect();

    // Write
    let mut writer = BufWriter::new(std::fs::File::create(out_file).unwrap());
    let mut count = 0;
    let mut append = |content: &[u8]| {
        let start = count;
        count += content.len();
        writer.write_all(&content).unwrap();
        (start as u64, content.len() as u32)
    };
    let items: Vec<_> = compressed
        .into_iter()
        .map(|(path, (etag, plain), gzip, brotli)| Item {
            path,
            plain: (etag, append(&plain)),
            gzip: gzip.map(|(etag, gzip)| (etag, append(&gzip))),
            brotli: brotli.map(|(etag, brotli)| (etag, append(&brotli))),
        })
        .collect();
    writer.flush().unwrap();
    items
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
