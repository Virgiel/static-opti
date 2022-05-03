use std::path::Path;

use hashbrown::HashMap;
use memmap2::Mmap;

pub mod embeded;
pub mod worker;

/// Extract supported encoding and corresponding tag
fn match_encoding_tag<'a>(
    accept_encoding: &str,
    item: &Item<'a>,
) -> (Option<&'static str>, (&'a str, &'a [u8])) {
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
pub struct ReportItem<'a> {
    pub path: &'a str,
    pub plain: (&'a str, (u64, u32)),
    pub gzip: Option<(&'a str, (u64, u32))>,
    pub brotli: Option<(&'a str, (u64, u32))>,
}
pub struct Item<'a> {
    pub path: &'a str,
    pub plain: (&'a str, &'a [u8]),
    pub gzip: Option<(&'a str, &'a [u8])>,
    pub brotli: Option<(&'a str, &'a [u8])>,
}

impl<'a> Item<'a> {
    pub fn from_report(item: &ReportItem<'a>, content: &'a [u8]) -> Self {
        Self {
            path: item.path,
            plain: Self::borrow(item.plain, content),
            gzip: item.gzip.map(|it| Self::borrow(it, content)),
            brotli: item.brotli.map(|it| Self::borrow(it, content)),
        }
    }

    fn borrow(it: (&'a str, (u64, u32)), content: &'a [u8]) -> (&'a str, &'a [u8]) {
        let (str, (start, len)) = it;
        (str, &content[start as usize..][..len as usize])
    }
}

pub struct Match<'a> {
    pub path: &'a str,
    pub content: &'a [u8],
    pub etag: &'a str,
    pub encoding: Option<&'a str>,
}

pub struct FileService<'a> {
    map: HashMap<&'a str, Item<'a>>,
}

impl<'a> FileService<'a> {
    /// Create and optimized file service at runtime and leak its mapped content
    pub fn build(static_dir: impl AsRef<Path>) -> Self {
        let dir = tempfile::TempDir::new().unwrap();
        worker::optimize(static_dir.as_ref(), dir.path());
        Self::leak(dir.path())
    }

    /// Create a file service from a dir by leaking its mapped content
    pub fn leak(dir: &Path) -> Self {
        let content: &'static Mmap = Box::leak(Box::new(unsafe {
            Mmap::map(&std::fs::File::open(dir.join("out.static")).unwrap()).unwrap()
        }));
        let report: &'static Mmap = Box::leak(Box::new(unsafe {
            Mmap::map(&std::fs::File::open(dir.join("report.json")).unwrap()).unwrap()
        }));
        Self::from_raw(content, report)
    }

    /// Create a file service from static ressources
    pub fn from_raw(content: &'a [u8], report: &'a [u8]) -> Self {
        let items: Vec<ReportItem> = serde_json::from_slice(report).unwrap();
        Self {
            map: HashMap::from_iter(
                items
                    .into_iter()
                    .map(|it| (it.path, Item::from_report(&it, content))),
            ),
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
    fn match_item(&self, accept_encoding: &str, item: &Item<'a>) -> Match {
        let (encoding, (etag, content)) = match_encoding_tag(accept_encoding, item);
        Match {
            path: &item.path,
            content,
            etag,
            encoding,
        }
    }
}
