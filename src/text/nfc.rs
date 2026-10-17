use unicode_normalization::char::{canonical_combining_class, decompose_compatible};
use unicode_normalization::{IsNormalized, UnicodeNormalization, is_nfc_quick, is_nfc_stream_safe};

/// IsNFC reports whether s is in NFC. Text made only of runes that are always
/// NFC on their own and never combine with a neighbour (ASCII and Latin-1,
/// Cyrillic letters, dashes and quotes) is answered without the norm tables,
/// which are slow on long Cyrillic text.
pub fn is_nfc(s: &str) -> bool {
    if s.chars().all(stable_rune) {
        return true;
    }
    // Go's norm.NFC.IsNormalString also rejects more than 30 combining marks in a row (stream-safe).
    is_nfc_stream_safe(s)
}

/// NFC returns s in NFC.
pub fn nfc(s: &str) -> String {
    if is_nfc(s) {
        return s.to_string();
    }
    // Go inserts U+034F after 30 combining marks in a row, as stream_safe does.
    s.stream_safe().nfc().collect()
}

pub(crate) fn stable_rune(r: char) -> bool {
    let r = r as u32;
    r < 0x300 || (0x400..=0x482).contains(&r) || (0x48a..=0x4ff).contains(&r) || (0x2010..=0x2027).contains(&r)
}

/// boundary_before reports whether Go's norm.Iter starts a new NFC segment at r:
/// r is a starter (its decomposition does not lead with a combining mark)
/// that never composes with the rune before it. U+FF9E and U+FF9F count as
/// combining marks in Go because their compatibility decomposition is one.
// ponytail: Go also splits a segment after 30 combining marks; not reproduced, the mapping is only coarser there.
fn boundary_before(r: char) -> bool {
    let mut lead = None;
    decompose_compatible(r, |d| {
        lead.get_or_insert(d);
    });
    canonical_combining_class(lead.unwrap_or(r)) == 0
        && !matches!(is_nfc_quick(std::iter::once(r)), IsNormalized::Maybe)
}

/// A function that maps a byte range of the NFC text back to the source.
pub type BackFn = Box<dyn Fn(usize, usize) -> (usize, usize)>;

/// NFCMap returns s in NFC and a function that maps a byte range of the NFC
/// text back to the range of s that holds it, so a match found in NFC is
/// reported at the right place in the source.
pub fn nfc_map(s: &str) -> (String, BackFn) {
    if is_nfc(s) {
        return (s.to_string(), Box::new(|a, b| (a, b)));
    }
    // seg_n[i] and seg_o[i] are the starts of the i-th normalization segment in the NFC text and in s.
    // Each segment is normalized on its own, as Go's norm.Iter does.
    let mut b = String::new();
    let (mut seg_n, mut seg_o) = (Vec::new(), Vec::new());
    let mut o = 0;
    let ends = s.char_indices().skip(1).filter(|&(_, r)| boundary_before(r)).map(|(i, _)| i);
    for e in ends.chain(std::iter::once(s.len())) {
        seg_n.push(b.len());
        seg_o.push(o);
        b.push_str(&nfc(&s[o..e]));
        o = e;
    }
    seg_n.push(b.len());
    seg_o.push(s.len());
    let back = move |start: usize, end: usize| {
        let k = seg_n.partition_point(|&x| x < start + 1) - 1; // the segment that holds the start
        (seg_o[k], seg_o[seg_n.partition_point(|&x| x < end)]) // the end of the segment that holds the end
    };
    (b, Box::new(back))
}

#[cfg(test)]
mod tests {
    use super::*;

    // A stable rune must be NFC alone and must not compose with a mark after it
    // or with a letter before it, otherwise the fast path of IsNFC is wrong.
    #[test]
    fn test_stable_runes() {
        let norm = |s: &str| s.nfc().collect::<String>();
        for r in (0u32..0x2100).filter_map(char::from_u32) {
            if !stable_rune(r) || (r as u32) < 0x20 {
                continue;
            }
            let s = r.to_string();
            assert!(unicode_normalization::is_nfc(&s), "{:04X} is not NFC", r as u32);
            let (a, i) = (format!("a{s}"), format!("и{s}"));
            assert!(norm(&a) == a && norm(&i) == i, "{:04X} composes with the letter before it", r as u32);
            assert_eq!(canonical_combining_class(r), 0, "{:04X} is a combining mark", r as u32);
        }
        assert!(!is_nfc("и\u{0306}") && nfc("и\u{0306}") == "й" && is_nfc("Заголовок — «текст»"), "IsNFC/NFC");
    }

    #[test]
    fn nfc_map_offsets() {
        let s = "Это новыи\u{0306} и cafe\u{0301}.";
        let (n, back) = nfc_map(s);
        assert_eq!(n, "Это новый и café.");
        let a = n.find("новый").unwrap();
        let (x, y) = back(a, a + "новый".len());
        assert_eq!(&s[x..y], "новыи\u{0306}");
        let a = n.find("café").unwrap();
        let (x, y) = back(a, a + "café".len());
        assert_eq!(&s[x..y], "cafe\u{0301}");
    }
}
