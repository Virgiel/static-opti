use std::{io::Write, path::PathBuf, time::Instant};

use mimalloc::MiMalloc;
use static_opti::compress_merge;

#[global_allocator]
static GLOBAL: MiMalloc = MiMalloc;

fn main() {
    let start = Instant::now();
    let in_dir = std::env::args().nth(1).unwrap();
    let in_dir = PathBuf::from(in_dir);
    let out_dir = std::env::args().nth(2).unwrap();
    let out_dir = PathBuf::from(out_dir);
    std::fs::remove_dir_all(&out_dir).ok();
    std::fs::create_dir_all(&out_dir).unwrap();

    let out_file = out_dir.join("out.static");
    let items = compress_merge(in_dir, &out_file);
    // Write report
    std::fs::write(
        out_dir.join("report.json"),
        &serde_json::to_vec(&items).unwrap(),
    )
    .unwrap();

    // Print stats
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
    for item in &items {
        let plain = item.plain.1 .1;
        write!(
            &mut stdout,
            "{:<2$} {:>7}  ",
            item.path,
            format_size(plain as f32),
            max
        )
        .unwrap();
        for opt in [&item.gzip, &item.brotli] {
            if let Some((_, (_, len))) = opt {
                write!(
                    &mut stdout,
                    "{:>7} {}%  ",
                    format_size(*len as f32),
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
        items.len(),
        format_size(std::fs::metadata(out_file).unwrap().len() as f32),
        start.elapsed()
    )
    .unwrap();
}

/// Format byte size in an human readable format
fn format_size(mut size: f32) -> String {
    for symbol in &[" B", "kB", "MB", "GB", "TB"] {
        if size < 1000. {
            return format!("{:.1}{}", size, symbol);
        } else {
            size /= 1024.;
        }
    }
    return format!("{:.1}TB", size);
}
