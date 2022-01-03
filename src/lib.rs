use std::path::PathBuf;

use hashbrown::HashMap;
use memmap2::Mmap;

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
    /// Create a file service match from an item and an accept encoding header value
    pub fn new(static_dir: impl Into<PathBuf>) -> Self {
        let dir = static_dir.into();
        let bytes = std::fs::read(dir.join("report.json")).unwrap();
        let items: Vec<Item> = serde_json::from_slice(&bytes).unwrap();

        let mmap =
            unsafe { Mmap::map(&std::fs::File::open(dir.join("out.static")).unwrap()).unwrap() };
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

        let path = if path == "" {
            "index.html".to_string()
        } else {
            format!("{}/index.html", path)
        };

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

