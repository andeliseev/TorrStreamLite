use axum::http::{HeaderMap, header};

#[derive(Debug, PartialEq)]
pub struct ByteRange {
    pub start: u64,
    pub len: u64,
    pub partial: bool,
}

pub fn parse(headers: &HeaderMap, size: u64) -> Result<ByteRange, ()> {
    let Some(value) = headers.get(header::RANGE) else {
        return Ok(ByteRange {
            start: 0,
            len: size,
            partial: false,
        });
    };
    let s = value
        .to_str()
        .map_err(|_| ())?
        .strip_prefix("bytes=")
        .ok_or(())?;
    if size == 0 || s.contains(',') {
        return Err(());
    }
    let (a, b) = s.split_once('-').ok_or(())?;
    if a.is_empty() {
        let suffix: u64 = b.parse().map_err(|_| ())?;
        if suffix == 0 {
            return Err(());
        }
        let len = suffix.min(size);
        return Ok(ByteRange {
            start: size - len,
            len,
            partial: true,
        });
    }
    let start: u64 = a.parse().map_err(|_| ())?;
    if start >= size {
        return Err(());
    }
    let end = if b.is_empty() {
        size - 1
    } else {
        b.parse::<u64>().map_err(|_| ())?.min(size - 1)
    };
    if end < start {
        return Err(());
    }
    Ok(ByteRange {
        start,
        len: end - start + 1,
        partial: true,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn range(s: &str, n: u64) -> Result<ByteRange, ()> {
        let mut h = HeaderMap::new();
        h.insert(header::RANGE, s.parse().unwrap());
        parse(&h, n)
    }
    #[test]
    fn supports_seek_suffix_and_clamping() {
        assert_eq!(
            range("bytes=50-", 100).unwrap(),
            ByteRange {
                start: 50,
                len: 50,
                partial: true
            }
        );
        assert_eq!(
            range("bytes=-20", 100).unwrap(),
            ByteRange {
                start: 80,
                len: 20,
                partial: true
            }
        );
        assert_eq!(range("bytes=90-999", 100).unwrap().len, 10);
        assert_eq!(range("bytes=-200", 100).unwrap().len, 100);
    }
    #[test]
    fn rejects_invalid_and_multiple_ranges() {
        for s in [
            "bytes=100-",
            "bytes=7-2",
            "bytes=-0",
            "bytes=0-2,5-8",
            "bytes=",
            "items=0-2",
            "bytes=18446744073709551616-",
        ] {
            assert!(range(s, 100).is_err(), "{s}");
        }
        assert!(range("bytes=0-", 0).is_err());
    }
}
