use std::{
    fmt::{Display, Formatter},
    io::{BufWriter, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicUsize, Ordering},
    time::Instant,
};

use libdeflater::{CompressionLvl, Compressor};
use mimalloc::MiMalloc;
use rayon::iter::{IntoParallelRefIterator, ParallelIterator};
use static_opti::Item;
use status_line::StatusLine;

#[global_allocator]
static GLOBAL: MiMalloc = MiMalloc;

fn main() {
    let start = Instant::now();
    let in_dir = std::env::args().nth(1).unwrap();
    let in_dir = PathBuf::from(in_dir);
    let out_dir = std::env::args().nth(2).unwrap();
    let out_dir = PathBuf::from(out_dir);
    let mut entries = Vec::new();
    std::fs::remove_dir_all(&out_dir).unwrap();
    std::fs::create_dir_all(&out_dir).unwrap();

    walk(&in_dir, &mut entries);

    // Parallel compression
    let status = StatusLine::new(Progress("compression", AtomicUsize::new(0), entries.len()));
    let compressed: Vec<_> = entries
        .par_iter()
        .map(|p| {
            // Read plain file
            let plain = std::fs::read(&p).unwrap();
            // Format path
            let path = p
                .strip_prefix(&in_dir)
                .unwrap()
                .to_str()
                .unwrap()
                .to_string()
                .replace("\\", "/");
            // Gzip compress
            let mut compressor = Compressor::new(CompressionLvl::best());
            let max_size = compressor.gzip_compress_bound(plain.len());
            let mut gzip = vec![0; max_size];
            let gzip_size = compressor.gzip_compress(&plain, &mut gzip).unwrap();
            gzip.resize(gzip_size, 0);
            let gzip = (gzip.len() * 100 / plain.len() < 90).then(|| (etag(&gzip), gzip));

            // Brotli compress
            let mut brotli = Vec::new();
            let mut writer = brotli::CompressorWriter::new(&mut brotli, 4096, 11, 24);
            writer.write_all(&plain).unwrap();
            writer.flush().unwrap();
            writer.into_inner();
            let brotli = (brotli.len() * 100 / plain.len() < 90).then(|| (etag(&brotli), brotli));

            // Increment status
            status.increment();

            (path, (etag(&plain), plain), gzip, brotli)
        })
        .collect();

    // Write
    let mut writer = BufWriter::new(std::fs::File::create(out_dir.join("out.static")).unwrap());
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

    // Write report
    std::fs::write(
        out_dir.join("report.json"),
        &serde_json::to_vec(&items).unwrap(),
    )
    .unwrap();

    // Print stats
    drop(status);
    let max = items.iter().map(|t| t.path.len()).max().unwrap_or(0);
    let stdout = std::io::stdout();
    let mut stdout = stdout.lock();
    writeln!(
        &mut stdout,
        "{:<2$}  Plain       Gzip        Brotli\n{:-<3$}",
        "Name",
        "",
        max,
        max + 26
    )
    .unwrap();
    for item in items {
        let plain = item.plain.1 .1;
        write!(
            &mut stdout,
            "{:<2$} {:>7}  ",
            item.path,
            format_size(plain as f32),
            max
        )
        .unwrap();
        for opt in [item.gzip, item.brotli] {
            if let Some((_, (_, len))) = opt {
                write!(
                    &mut stdout,
                    "{:>7} {}%  ",
                    format_size(len as f32),
                    (len * 100 / plain),
                )
                .unwrap();
            } else {
                write!(&mut stdout, "             ").unwrap();
            }
        }
        writeln!(&mut stdout, "").unwrap();
    }
    writeln!(
        &mut stdout,
        "\n Optimized {} files to {} in {:?}",
        entries.len(),
        format_size(count as f32),
        start.elapsed()
    )
    .unwrap();
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

/// Format byte size in an human readable format
fn format_size(mut size: f32) -> String {
    for symbol in &[" B", "kB", "MB", "GB", "TB"] {
        if size < 1024. {
            return format!("{:.1}{}", size, symbol);
        } else {
            size /= 1024.;
        }
    }
    return format!("{:.1}TB", size);
}

struct Progress(&'static str, AtomicUsize, usize);

impl Progress {
    fn increment(&self) {
        self.1.fetch_add(1, Ordering::SeqCst);
    }
}

impl Display for Progress {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let progress = self.1.load(Ordering::SeqCst);
        write!(
            f,
            "{}: {}% {}/{}",
            self.0,
            progress * 100 / self.2,
            progress,
            self.2
        )
    }
}
