use std::{io::Write, path::PathBuf, time::Instant};

use clap::Parser;
use mimalloc::MiMalloc;
use static_opti::worker::optimize;

#[global_allocator]
static GLOBAL: MiMalloc = MiMalloc;

/// Prepare static files for efficient serving
#[derive(Parser, Debug)]
#[clap(long_about = None)]
struct Args {
    /// The directory containing static files
    in_dir: PathBuf,
    /// The path where to put the output
    out: PathBuf,
}

fn main() {
    let start = Instant::now();
    let args = Args::parse();

    let (_, items) = optimize(&args.in_dir, &args.out);

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
        max + 34
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
                    (100 - (len * 100 / plain)),
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
        "\n Optimized {} files in {:?}",
        items.len(),
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
