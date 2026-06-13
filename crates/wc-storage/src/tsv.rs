use std::cmp::Ordering;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TsvRow {
    pub ftype: String,
    pub ext: String,
    pub backend: String,
    pub size: u64,
    pub mtime: u64,
    pub resolution: String,
    pub path: String,
}

pub fn parse_tsv_line(line: &str) -> Option<TsvRow> {
    let parts: Vec<&str> = line.split('\t').collect();
    if parts.len() < 7 {
        return None;
    }
    Some(TsvRow {
        ftype: parts[0].to_string(),
        ext: parts[1].to_string(),
        backend: parts[2].to_string(),
        size: parts[3].parse().unwrap_or(0),
        mtime: parts[4].parse().unwrap_or(0),
        resolution: parts[5].to_string(),
        path: parts[6].to_string(),
    })
}

fn matches_filter(row: &TsvRow, filter: &str) -> bool {
    match filter {
        "image" | "images" => row.ftype == "image",
        "gif" | "gifs" => row.ftype == "gif",
        "video" | "videos" => row.ftype == "video",
        "we" => row.ftype == "we_scene" || row.ftype == "we_web",
        "we_scene" => row.ftype == "we_scene",
        "we_web" => row.ftype == "we_web",
        "unsupported" => row.ftype == "unsupported",
        _ => true,
    }
}

fn matches_search(row: &TsvRow, search: &str) -> bool {
    let q = search.trim().to_lowercase();
    q.is_empty() || row.path.to_lowercase().contains(&q)
}

pub fn compare_rows(a: &TsvRow, b: &TsvRow, sort: &str) -> Ordering {
    match sort {
        "name" => a.path.cmp(&b.path),
        "size" => b.size.cmp(&a.size).then(a.path.cmp(&b.path)),
        _ => b.mtime.cmp(&a.mtime).then(a.path.cmp(&b.path)),
    }
}

pub fn tsv_bounded_page(
    path: &Path,
    filter: &str,
    sort: &str,
    search: &str,
    offset: usize,
    limit: usize,
) -> (usize, Vec<TsvRow>) {
    let file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(_) => return (0, Vec::new()),
    };
    let reader = std::io::BufReader::new(file);
    let mut rows = Vec::new();
    for line in std::io::BufRead::lines(reader).map_while(Result::ok) {
        let Some(row) = parse_tsv_line(&line) else {
            continue;
        };
        if matches_filter(&row, filter) && matches_search(&row, search) {
            rows.push(row);
        }
    }
    rows.sort_by(|a, b| compare_rows(a, b, sort));
    let total = rows.len();
    let page = if limit == 0 {
        Vec::new()
    } else {
        rows.into_iter().skip(offset).take(limit).collect()
    };
    (total, page)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn limit_zero_counts_without_page() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(
            tmp.path(),
            "image\tjpg\tawww\t1\t1\t?x?\t/a.jpg\nvideo\tmp4\tmpvpaper\t2\t2\t?x?\t/b.mp4\n",
        )
        .unwrap();
        let (total, rows) = tsv_bounded_page(tmp.path(), "all", "mtime", "", 0, 0);
        assert_eq!(total, 2);
        assert!(rows.is_empty());
    }
}
