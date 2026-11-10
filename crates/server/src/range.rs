/// One byte range only; invalid and multipart ranges fail explicitly.
pub fn parse(value: &str, size: u64) -> Result<(u64, u64), ()> {
    let v = value.strip_prefix("bytes=").ok_or(())?;
    if size == 0 || v.contains(',') {
        return Err(());
    }
    let (start, end) = v.split_once('-').ok_or(())?;
    if start.is_empty() {
        let suffix: u64 = end.parse().map_err(|_| ())?;
        if suffix == 0 {
            return Err(());
        }
        return Ok((size.saturating_sub(suffix), size - 1));
    }
    let start: u64 = start.parse().map_err(|_| ())?;
    let end: u64 = if end.is_empty() {
        size - 1
    } else {
        end.parse().map_err(|_| ())?
    };
    if start >= size || end < start {
        return Err(());
    }
    Ok((start, end.min(size - 1)))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ranges() {
        assert_eq!(parse("bytes=5-9", 20), Ok((5, 9)));
        assert_eq!(parse("bytes=5-", 20), Ok((5, 19)));
        assert_eq!(parse("bytes=-5", 20), Ok((15, 19)));
        assert_eq!(parse("bytes=-100", 20), Ok((0, 19)));
        for v in [
            "bytes=20-",
            "bytes=6-3",
            "bytes=-0",
            "bytes=0-1,3-4",
            "bytes=x-",
            "other=0-1",
        ] {
            assert!(parse(v, 20).is_err());
        }
        assert!(parse("bytes=0-", 0).is_err());
    }
}
