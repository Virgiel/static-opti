//! Static version from embemed
use std::{
    io::{BufWriter, Write},
    path::Path,
};

use hashbrown::HashMap;

use crate::compress_merge;

/// Extract supported encoding and corresponding tag
fn match_encoding_tag(
    accept_encoding: &str,
    item: &Item,
) -> (Option<&'static str>, (&'static str, (u64, u32))) {
    if let Some(it) = &item.brotli {
        if accept_encoding.contains("br") {
            return (Some("br"), *it);
        }
    }
    if let Some(it) = &item.gzip {
        if accept_encoding.contains("gzip") {
            return (Some("gzip"), *it);
        }
    }
    (None, item.plain)
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct Item {
    pub path: &'static str,
    pub plain: (&'static str, (u64, u32)),
    pub gzip: Option<(&'static str, (u64, u32))>,
    pub brotli: Option<(&'static str, (u64, u32))>,
}

pub struct Match {
    pub path: &'static str,
    pub content: &'static [u8],
    pub etag: &'static str,
    pub encoding: Option<&'static str>,
}

pub struct FileService {
    content: &'static [u8],
    map: HashMap<&'static str, Item>,
}

impl FileService {
    /// Create a file service from static ressources
    pub fn from_raw(content: &'static [u8], report: &'static [u8]) -> Self {
        let items: Vec<Item> = serde_json::from_slice(report).unwrap();
        Self {
            content,
            map: HashMap::from_iter(items.into_iter().map(|it| (it.path, it))),
        }
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

            if let Some(it) = self.map.get(path.as_str()) {
                return Some(self.match_item(accept_encoding, it));
            }
        }

        let path = format!("{}.html", path);

        if let Some(it) = self.map.get(path.as_str()) {
            return Some(self.match_item(accept_encoding, it));
        }

        return None;
    }

    /// Construct match from an item and an accept encoding header value
    fn match_item(&self, accept_encoding: &str, item: &Item) -> Match {
        let (encoding, (etag, (start, len))) = match_encoding_tag(accept_encoding, item);
        Match {
            path: &item.path,
            content: &self.content[start as usize..][..len as usize],
            etag,
            encoding,
        }
    }
}

pub fn codegen(path: &str, name: &str) {
    let out = std::env::var_os("OUT_DIR").unwrap();
    let out_dir: &Path = out.as_ref();
    println!("cargo:rerun-if-changed={}", path);

    // Generate static file
    let items = compress_merge(path).persist(&out_dir.join(format!("{name}.static")));
    let mut file = std::fs::File::create(&out_dir.join(format!("{name}.json"))).unwrap();
    serde_json::to_writer(&mut file, &items).unwrap();

    let mut rust_file =
        BufWriter::new(std::fs::File::create(&out_dir.join(format!("{name}.rs"))).unwrap());
    // Generate rust file
    writeln!(
        &mut rust_file,
        "use static_opti::embeded::FileService;\n\npub fn static_load() -> FileService {{\n\tFileService::from_raw(include_bytes!(\"{name}.static\"),include_bytes!(\"{name}.json\"))\n}}"
    )
    .unwrap();
    rust_file.into_inner().unwrap();
}
