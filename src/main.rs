use std::{io::Write, time::Instant};

use mimalloc::MiMalloc;
use static_opti::optimize;

#[global_allocator]
static GLOBAL: MiMalloc = MiMalloc;

fn main() {
    let start = Instant::now();
    let in_dir = std::env::args_os().nth(1).unwrap();
    let out_dir = std::env::args_os().nth(2).unwrap();

    let items = optimize(in_dir.as_ref(), out_dir.as_ref());

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
