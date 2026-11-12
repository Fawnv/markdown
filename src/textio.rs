//! Reading and writing text files the way Notepad does: BOM sniffing, UTF-8,
//! UTF-16 LE/BE, legacy ANSI code page fallback, CRLF/LF preservation.

use encoding_rs::Encoding;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Enc {
    Utf8 { bom: bool },
    Utf16Le,
    Utf16Be,
    Legacy(&'static Encoding),
}

impl Enc {
    pub fn label(self) -> String {
        match self {
            Enc::Utf8 { bom: false } => "UTF-8".into(),
            Enc::Utf8 { bom: true } => "UTF-8 BOM".into(),
            Enc::Utf16Le => "UTF-16 LE".into(),
            Enc::Utf16Be => "UTF-16 BE".into(),
            Enc::Legacy(e) => e.name().to_string(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Eol {
    Crlf,
    Lf,
}

impl Eol {
    pub fn as_str(self) -> &'static str {
        match self {
            Eol::Crlf => "crlf",
            Eol::Lf => "lf",
        }
    }
    pub fn parse(s: &str) -> Eol {
        if s.eq_ignore_ascii_case("lf") { Eol::Lf } else { Eol::Crlf }
    }
}

pub struct Decoded {
    /// Text with `\n` line endings only.
    pub text: String,
    pub enc: Enc,
    pub eol: Eol,
}

pub fn decode(bytes: &[u8]) -> Decoded {
    let (raw, enc): (String, Enc) = if let Some(rest) = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        (String::from_utf8_lossy(rest).into_owned(), Enc::Utf8 { bom: true })
    } else if let Some(rest) = bytes.strip_prefix(&[0xFF, 0xFE]) {
        (decode_utf16(rest, true), Enc::Utf16Le)
    } else if let Some(rest) = bytes.strip_prefix(&[0xFE, 0xFF]) {
        (decode_utf16(rest, false), Enc::Utf16Be)
    } else if let Ok(s) = std::str::from_utf8(bytes) {
        (s.to_owned(), Enc::Utf8 { bom: false })
    } else if looks_like_utf16le(bytes) {
        (decode_utf16(bytes, true), Enc::Utf16Le)
    } else {
        let e = ansi_encoding();
        let (s, _) = e.decode_without_bom_handling(bytes);
        (s.into_owned(), Enc::Legacy(e))
    };

    let crlf = raw.matches("\r\n").count();
    let lf = raw.matches('\n').count() - crlf;
    let eol = if crlf > 0 && crlf >= lf {
        Eol::Crlf
    } else if lf > 0 {
        Eol::Lf
    } else {
        Eol::Crlf // Notepad default for new/one-line files
    };
    let text = raw.replace("\r\n", "\n").replace('\r', "\n");
    Decoded { text, enc, eol }
}

/// Encode for saving. Returns the bytes and the encoding actually used
/// (falls back to UTF-8 when the legacy code page can't represent the text).
pub fn encode(text: &str, enc: Enc, eol: Eol) -> (Vec<u8>, Enc) {
    let text: Cow<'_, str> = match eol {
        Eol::Crlf => Cow::Owned(text.replace('\n', "\r\n")),
        Eol::Lf => Cow::Borrowed(text),
    };
    match enc {
        Enc::Utf8 { bom } => {
            let mut v = Vec::with_capacity(text.len() + 3);
            if bom {
                v.extend_from_slice(&[0xEF, 0xBB, 0xBF]);
            }
            v.extend_from_slice(text.as_bytes());
            (v, enc)
        }
        Enc::Utf16Le | Enc::Utf16Be => {
            let le = enc == Enc::Utf16Le;
            let mut v = Vec::with_capacity(text.len() * 2 + 2);
            v.extend_from_slice(if le { &[0xFF, 0xFE] } else { &[0xFE, 0xFF] });
            for u in text.encode_utf16() {
                v.extend_from_slice(&if le { u.to_le_bytes() } else { u.to_be_bytes() });
            }
            (v, enc)
        }
        Enc::Legacy(e) => {
            let (bytes, _, had_errors) = e.encode(&text);
            if had_errors {
                (text.as_bytes().to_vec(), Enc::Utf8 { bom: false })
            } else {
                (bytes.into_owned(), enc)
            }
        }
    }
}

use std::borrow::Cow;

fn decode_utf16(bytes: &[u8], le: bool) -> String {
    let units = bytes.chunks_exact(2).map(|c| if le { u16::from_le_bytes([c[0], c[1]]) } else { u16::from_be_bytes([c[0], c[1]]) });
    char::decode_utf16(units).map(|r| r.unwrap_or('\u{FFFD}')).collect()
}

/// BOM-less UTF-16 LE (e.g. some PowerShell redirections): lots of zero high bytes.
fn looks_like_utf16le(bytes: &[u8]) -> bool {
    if bytes.len() < 4 || bytes.len() % 2 != 0 {
        return false;
    }
    let sample = &bytes[..bytes.len().min(4096)];
    let zeros_odd = sample.iter().skip(1).step_by(2).filter(|&&b| b == 0).count();
    let zeros_even = sample.iter().step_by(2).filter(|&&b| b == 0).count();
    zeros_odd * 10 > sample.len() / 2 * 7 && zeros_even * 10 < sample.len() / 2
}

/// The system ANSI code page (cp1251 on Russian Windows, cp1252 on English, ...).
pub fn ansi_encoding() -> &'static Encoding {
    #[cfg(windows)]
    let cp = unsafe { windows_sys::Win32::Globalization::GetACP() };
    #[cfg(not(windows))]
    let cp = 1252u32;
    use encoding_rs::*;
    match cp {
        874 => WINDOWS_874,
        932 => SHIFT_JIS,
        936 => GBK,
        949 => EUC_KR,
        950 => BIG5,
        866 => IBM866,
        1250 => WINDOWS_1250,
        1251 => WINDOWS_1251,
        1253 => WINDOWS_1253,
        1254 => WINDOWS_1254,
        1255 => WINDOWS_1255,
        1256 => WINDOWS_1256,
        1257 => WINDOWS_1257,
        1258 => WINDOWS_1258,
        20866 => KOI8_R,
        21866 => KOI8_U,
        65001 => UTF_8,
        _ => WINDOWS_1252,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_utf8_crlf() {
        let d = decode(b"a\r\nb\r\n");
        assert_eq!(d.text, "a\nb\n");
        assert_eq!(d.eol, Eol::Crlf);
        assert_eq!(d.enc, Enc::Utf8 { bom: false });
        assert_eq!(encode(&d.text, d.enc, d.eol).0, b"a\r\nb\r\n");
    }

    #[test]
    fn utf16_bom() {
        let mut b = vec![0xFF, 0xFE];
        for u in "Привет\r\n".encode_utf16() {
            b.extend_from_slice(&u.to_le_bytes());
        }
        let d = decode(&b);
        assert_eq!(d.text, "Привет\n");
        assert_eq!(d.enc, Enc::Utf16Le);
        assert_eq!(encode(&d.text, d.enc, d.eol).0, b);
    }

    #[test]
    fn legacy_cp1251() {
        let (bytes, _, _) = encoding_rs::WINDOWS_1251.encode("Привет");
        let d = decode(&bytes);
        if ansi_encoding() == encoding_rs::WINDOWS_1251 {
            assert_eq!(d.text, "Привет");
        }
        assert!(matches!(d.enc, Enc::Legacy(_)));
    }

    #[test]
    fn lf_detected() {
        assert_eq!(decode(b"a\nb\nc\r\n").eol, Eol::Lf);
    }
}
