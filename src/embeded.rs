use std::{
    io::{BufWriter, Write},
    path::Path,
};

use crate::worker::compress_dir;

pub fn codegen(path: &str, name: &str) {
    let out = std::env::var_os("OUT_DIR").unwrap();
    let out_dir: &Path = out.as_ref();
    println!("cargo:rerun-if-changed={}", path);

    // Generate static file
    let items = compress_dir(path).persist(&out_dir.join(format!("{name}.static")));
    let mut file = std::fs::File::create(&out_dir.join(format!("{name}.json"))).unwrap();
    serde_json::to_writer(&mut file, &items).unwrap();

    let mut rust_file =
        BufWriter::new(std::fs::File::create(&out_dir.join(format!("{name}.rs"))).unwrap());
    // Generate rust file
    writeln!(
        &mut rust_file,
        "use static_opti::FileService;\n\npub fn static_load() -> FileService<'static> {{\n\tFileService::from_raw(include_bytes!(\"{name}.static\"),include_bytes!(\"{name}.json\"))\n}}"
    )
    .unwrap();
    rust_file.into_inner().unwrap();
}
