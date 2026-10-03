//! Which code page a content file was written in, and the other spellings a file name
//! may have picked up on its way to the disk.
//!
//! OMSI is a Delphi program that reads its text files with the system's ANSI code page: a
//! Russian installation reads Windows-1251, a Polish or Czech one Windows-1250, a German
//! one Windows-1252. A mod is written for the code page of its author, so the LiAZ 5292 or
//! the Scania Citywide's Russian cockpit texts only read as Cyrillic on a Russian Windows;
//! read as 1252, the LiAZ called itself "ËèÀÇ 5292.20". neoOMSI looks at every file on
//! its own ([`detect`]), except on a Windows whose ANSI code page is a double-byte one
//! (Chinese, Japanese, Korean): there it reads what is not UTF-8 in that code page, as
//! OMSI does, since a hanzi folder name in an `ailists.cfg` reads as nothing else.
//!
//! File names have a second problem: a zip archive stores a name without its UTF-8 flag in
//! the OEM code page of the machine that made it (CP866 on a Russian one), and the tool
//! that unpacked it guessed another (the Scania's `верх.png` arrived as `óąÓň.png`, CP866
//! bytes read as CP852). [`name_variants`] lists the names a file may carry for one that a
//! content file asks for, so the lookup finds it all the same.

use encoding_rs::Encoding;

/// The code pages content is written in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodePage {
    Utf8,
    Windows1250,
    Windows1251,
    Windows1252,
    /// CP936, simplified Chinese.
    Gbk,
    /// CP950, traditional Chinese.
    Big5,
    /// CP932, Japanese.
    ShiftJis,
    /// CP949, Korean.
    EucKr,
}

impl CodePage {
    pub fn encoding(self) -> &'static Encoding {
        match self {
            CodePage::Utf8 => encoding_rs::UTF_8,
            CodePage::Windows1250 => encoding_rs::WINDOWS_1250,
            CodePage::Windows1251 => encoding_rs::WINDOWS_1251,
            CodePage::Windows1252 => encoding_rs::WINDOWS_1252,
            CodePage::Gbk => encoding_rs::GBK,
            CodePage::Big5 => encoding_rs::BIG5,
            CodePage::ShiftJis => encoding_rs::SHIFT_JIS,
            CodePage::EucKr => encoding_rs::EUC_KR,
        }
    }

    /// The double-byte code page a Windows ANSI code page number stands for.
    #[cfg_attr(not(any(windows, test)), allow(dead_code))]
    fn double_byte(acp: u32) -> Option<CodePage> {
        match acp {
            936 => Some(CodePage::Gbk),
            950 => Some(CodePage::Big5),
            932 => Some(CodePage::ShiftJis),
            949 => Some(CodePage::EucKr),
            _ => None,
        }
    }
}

/// The system's ANSI code page when it is a double-byte one (Windows only).
fn system_double_byte() -> Option<CodePage> {
    #[cfg(windows)]
    {
        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn GetACP() -> u32;
        }
        static ACP: std::sync::OnceLock<u32> = std::sync::OnceLock::new();
        // SAFETY: GetACP takes nothing and only returns a number.
        CodePage::double_byte(*ACP.get_or_init(|| unsafe { GetACP() }))
    }
    #[cfg(not(windows))]
    {
        None
    }
}

/// A byte that is a letter in Windows-1251 (А-я, Ё, ё).
fn cyrillic_letter(b: u8) -> bool {
    b >= 0xC0 || b == 0xA8 || b == 0xB8
}

/// Bytes that are letters in Windows-1250 but signs in 1252: lower case ł ą ś ź (³ ¹ œ Ÿ)
/// and capitals Ł Ą Ś Ź Ż (£ ¥ Œ  ¯).
fn central_european_lower(b: u8) -> bool {
    matches!(b, 0xB3 | 0xB9 | 0x9C | 0x9F)
}

fn central_european_upper(b: u8) -> bool {
    matches!(b, 0xA3 | 0xA5 | 0x8C | 0x8F | 0xAF)
}

/// The code page `bytes` (without a byte-order mark) were most likely written in.
///
/// * valid UTF-8 with anything beyond ASCII in it is UTF-8 (newer mods);
/// * Russian text is words of Cyrillic letters, i.e. runs of three and more bytes of
///   `0xC0..=0xFF`; a Western text never has three accented letters in a row (German has
///   at most two, "Größe"), so half of the high letters sitting in such runs means 1251;
/// * a Polish or Czech text has 1250 letters that are signs in 1252 next to plain letters;
/// * everything else is Windows-1252, the code page of the stock content.
///
/// On a Windows with a double-byte ANSI code page, what is not UTF-8 and reads in that one
/// without a broken character is in it.
pub fn detect(bytes: &[u8]) -> CodePage {
    detect_on(bytes, system_double_byte())
}

fn detect_on(bytes: &[u8], system: Option<CodePage>) -> CodePage {
    if bytes.is_ascii() {
        return CodePage::Windows1252;
    }
    if std::str::from_utf8(bytes).is_ok() {
        return CodePage::Utf8;
    }
    // what reads as that code page without a broken character is in it; Russian content on
    // a Chinese Windows almost never does (a Cyrillic word of odd length leaves
    // a lead byte before a space), and read as GBK all the same, a Russian HOF's stops lost
    // their names and no longer matched the map's
    if let Some(page) = system {
        if page
            .encoding()
            .decode_without_bom_handling_and_without_replacement(bytes)
            .is_some()
        {
            return page;
        }
    }
    let (mut letters, mut in_runs, mut run) = (0usize, 0usize, 0usize);
    let close_run = |run: &mut usize, in_runs: &mut usize| {
        if *run >= 3 {
            *in_runs += *run;
        }
        *run = 0;
    };
    for &b in bytes {
        if cyrillic_letter(b) {
            letters += 1;
            run += 1;
        } else {
            close_run(&mut run, &mut in_runs);
        }
    }
    close_run(&mut run, &mut in_runs);
    if in_runs >= 3 && in_runs * 2 >= letters {
        return CodePage::Windows1251;
    }
    // a lower-case one between two letters ("Głowny"), a capital before one ("Łazarz");
    // "m³/s" in the stock constfiles is neither
    let letter = |i: Option<usize>| {
        i.and_then(|i| bytes.get(i))
            .map(|b| b.is_ascii_alphabetic() || *b >= 0xC0)
            .unwrap_or(false)
    };
    let central = bytes
        .iter()
        .enumerate()
        .filter(|(i, b)| {
            let (before, after) = (letter(i.checked_sub(1)), letter(Some(i + 1)));
            (central_european_lower(**b) && before && after)
                || (central_european_upper(**b) && after)
        })
        .count();
    if central >= 2 {
        return CodePage::Windows1250;
    }
    CodePage::Windows1252
}

/// `bytes` decoded in the code page [`detect`] finds.
pub fn decode(bytes: &[u8]) -> String {
    if bytes.is_ascii() {
        // ASCII is ASCII in every code page (and the common case by far)
        return String::from_utf8_lossy(bytes).into_owned();
    }
    let (s, _) = detect(bytes).encoding().decode_without_bom_handling(bytes);
    s.into_owned()
}

/// Code page 437, which zip tools use for names without the UTF-8 flag.
pub(crate) const CP437_HIGH: [char; 128] = [
    'Ç', 'ü', 'é', 'â', 'ä', 'à', 'å', 'ç', 'ê', 'ë', 'è', 'ï', 'î', 'ì', 'Ä', 'Å', 'É', 'æ', 'Æ',
    'ô', 'ö', 'ò', 'û', 'ù', 'ÿ', 'Ö', 'Ü', '¢', '£', '¥', '₧', 'ƒ', 'á', 'í', 'ó', 'ú', 'ñ', 'Ñ',
    'ª', 'º', '¿', '⌐', '¬', '½', '¼', '¡', '«', '»', '░', '▒', '▓', '│', '┤', '╡', '╢', '╖', '╕',
    '╣', '║', '╗', '╝', '╜', '╛', '┐', '└', '┴', '┬', '├', '─', '┼', '╞', '╟', '╚', '╔', '╩', '╦',
    '╠', '═', '╬', '╧', '╨', '╤', '╥', '╙', '╘', '╒', '╓', '╫', '╪', '┘', '┌', '█', '▄', '▌', '▐',
    '▀', 'α', 'ß', 'Γ', 'π', 'Σ', 'σ', 'µ', 'τ', 'Φ', 'Θ', 'Ω', 'δ', '∞', 'φ', 'ε', '∩', '≡', '±',
    '≥', '≤', '⌠', '⌡', '÷', '≈', '°', '∙', '·', '√', 'ⁿ', '²', '■', '\u{a0}',
];

/// Code page 852 (DOS Central European), which unpackers guess for such names too.
const CP852_HIGH: [char; 128] = [
    'Ç', 'ü', 'é', 'â', 'ä', 'ů', 'ć', 'ç', 'ł', 'ë', 'Ő', 'ő', 'î', 'Ź', 'Ä', 'Ć', 'É', 'Ĺ', 'ĺ',
    'ô', 'ö', 'Ľ', 'ľ', 'Ś', 'ś', 'Ö', 'Ü', 'Ť', 'ť', 'Ł', '×', 'č', 'á', 'í', 'ó', 'ú', 'Ą', 'ą',
    'Ž', 'ž', 'Ę', 'ę', '¬', 'ź', 'Č', 'ş', '«', '»', '░', '▒', '▓', '│', '┤', 'Á', 'Â', 'Ě', 'Ş',
    '╣', '║', '╗', '╝', 'Ż', 'ż', '┐', '└', '┴', '┬', '├', '─', '┼', 'Ă', 'ă', '╚', '╔', '╩', '╦',
    '╠', '═', '╬', '¤', 'đ', 'Đ', 'Ď', 'Ë', 'ď', 'Ň', 'Í', 'Î', 'ě', '┘', '┌', '█', '▄', 'Ţ', 'Ů',
    '▀', 'Ó', 'ß', 'Ô', 'Ń', 'ń', 'ň', 'Š', 'š', 'Ŕ', 'Ú', 'ŕ', 'Ű', 'ý', 'Ý', 'ţ', '´', '\u{ad}',
    '˝', '˛', 'ˇ', '˘', '§', '÷', '¸', '°', '¨', '˙', 'ű', 'Ř', 'ř', '■', '\u{a0}',
];

/// A single-byte code page, as a way to turn a name back into the bytes it was made of
/// and to read bytes again.
#[derive(Clone, Copy)]
enum Single {
    Table(&'static [char; 128]),
    Enc(&'static Encoding),
}

impl Single {
    fn encode(self, s: &str) -> Option<Vec<u8>> {
        match self {
            Single::Table(t) => s
                .chars()
                .map(|c| {
                    if c.is_ascii() {
                        Some(c as u8)
                    } else {
                        t.iter().position(|x| *x == c).map(|i| 0x80 + i as u8)
                    }
                })
                .collect(),
            Single::Enc(e) => {
                let (b, _, unmappable) = e.encode(s);
                (!unmappable).then(|| b.into_owned())
            }
        }
    }

    fn decode(self, b: &[u8]) -> String {
        match self {
            Single::Table(t) => b
                .iter()
                .map(|&x| {
                    if x < 0x80 {
                        x as char
                    } else {
                        t[(x - 0x80) as usize]
                    }
                })
                .collect(),
            Single::Enc(e) => e.decode_without_bom_handling(b).0.into_owned(),
        }
    }
}

/// The spellings a file name may have picked up between its author's machine and this
/// one: `name` turned back into bytes in a code page it may have been read in wrongly
/// (the zip OEM pages 437 and 852, Windows 1252 and 1250) and read again in the one it
/// may have been written in (CP866, the Russian OEM page zip tools use, and Windows 1251,
/// 1250, 1252). ASCII names have no other spelling; `name` itself is not in the list.
pub fn name_variants(name: &str) -> Vec<String> {
    if name.is_ascii() {
        return Vec::new();
    }
    let read_as = [
        Single::Table(&CP437_HIGH),
        Single::Table(&CP852_HIGH),
        Single::Enc(encoding_rs::WINDOWS_1252),
        Single::Enc(encoding_rs::WINDOWS_1250),
        Single::Enc(encoding_rs::WINDOWS_1251),
    ];
    let written_in = [
        Single::Enc(encoding_rs::IBM866),
        Single::Enc(encoding_rs::WINDOWS_1251),
        Single::Enc(encoding_rs::WINDOWS_1250),
        Single::Enc(encoding_rs::WINDOWS_1252),
        Single::Table(&CP437_HIGH),
    ];
    let mut out: Vec<String> = Vec::new();
    for wrong in read_as {
        let Some(bytes) = wrong.encode(name) else {
            continue;
        };
        for right in written_in {
            let v = right.decode(&bytes);
            if v != name && !out.contains(&v) {
                out.push(v);
            }
        }
    }
    out
}

/// The same character in the other code pages content is written in: a font made on a
/// Russian machine lists `Л` as the byte 0xCB, which reads as `Ë` in 1252, and a text read
/// in the other code page must still find its letters in it.
pub fn char_variants(c: char) -> Vec<char> {
    if c.is_ascii() {
        return Vec::new();
    }
    let pages = [
        encoding_rs::WINDOWS_1251,
        encoding_rs::WINDOWS_1252,
        encoding_rs::WINDOWS_1250,
    ];
    let mut buf = [0u8; 4];
    let s: &str = c.encode_utf8(&mut buf);
    let mut out = Vec::new();
    for from in pages {
        let (b, _, bad) = from.encode(s);
        if bad || b.len() != 1 {
            continue;
        }
        for to in pages {
            if std::ptr::eq(from, to) {
                continue;
            }
            if let Some(v) = to.decode_without_bom_handling(&b).0.chars().next() {
                if v != c && v != '\u{fffd}' && !out.contains(&v) {
                    out.push(v);
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cp1251(s: &str) -> Vec<u8> {
        encoding_rs::WINDOWS_1251.encode(s).0.into_owned()
    }

    fn cp1252(s: &str) -> Vec<u8> {
        encoding_rs::WINDOWS_1252.encode(s).0.into_owned()
    }

    #[test]
    fn detects_the_code_page() {
        assert_eq!(
            detect(&cp1251(
                "[friendlyname]\r\nЛиАЗ\r\n5292.20\r\nЗаводская\r\n"
            )),
            CodePage::Windows1251
        );
        assert_eq!(detect(&cp1251("верх.png")), CodePage::Windows1251);
        // German never has three accented letters in a row, even with "Größe"
        assert_eq!(
            detect(&cp1252("Größe der Straße, Bahnübergang, Müllerstraße")),
            CodePage::Windows1252
        );
        assert_eq!(
            detect(&cp1252("'(c) Rüdiger Hülsmann\r\n{trigger:a}\r\n")),
            CodePage::Windows1252
        );
        let pl = encoding_rs::WINDOWS_1250
            .encode("Łazarz, Śródka, Żegrze, Dworzec Główny")
            .0
            .into_owned();
        assert_eq!(detect(&pl), CodePage::Windows1250);
        assert_eq!(
            detect(&cp1252("Volumenstrom in m³/s, Dichte in g/m³")),
            CodePage::Windows1252
        );
        assert_eq!(detect("Überlandbus".as_bytes()), CodePage::Utf8);
        assert_eq!(decode(&cp1251("ЛиАЗ")), "ЛиАЗ");
    }

    #[test]
    fn reads_the_double_byte_system_code_page() {
        let gbk = encoding_rs::GBK
            .encode("Vehicles\\公交车\\x.bus")
            .0
            .into_owned();
        let page = detect_on(&gbk, CodePage::double_byte(936));
        assert_eq!(page, CodePage::Gbk);
        assert_eq!(
            page.encoding().decode_without_bom_handling(&gbk).0,
            "Vehicles\\公交车\\x.bus"
        );
        // UTF-8 stays UTF-8, and other systems keep the guess
        assert_eq!(
            detect_on("公交车".as_bytes(), CodePage::double_byte(936)),
            CodePage::Utf8
        );
        assert_ne!(detect_on(&gbk, CodePage::double_byte(1251)), CodePage::Gbk);
        // a Russian file on a Chinese system is not GBK
        let ru = cp1251("[station]\r\nУлица Ленина\r\nМетро Сокол\r\n");
        assert_eq!(
            detect_on(&ru, CodePage::double_byte(936)),
            CodePage::Windows1251
        );
    }

    #[test]
    fn finds_misread_file_names() {
        // CP866 bytes read as CP852 by the unpacker
        assert!(name_variants("óąÓň.png").contains(&"верх.png".to_string()));
        // the same read as CP437 inside a zip
        let cp437: String = encoding_rs::IBM866
            .encode("верх")
            .0
            .iter()
            .map(|&b| CP437_HIGH[(b - 0x80) as usize])
            .collect();
        assert!(name_variants(&cp437).contains(&"верх".to_string()));
        // a 1251 name that was read as 1252
        assert!(name_variants("âåðõ.png").contains(&"верх.png".to_string()));
        assert!(name_variants("plain.png").is_empty());
    }

    #[test]
    fn maps_letters_between_code_pages() {
        assert!(char_variants('Л').contains(&'Ë'));
        assert!(char_variants('Ë').contains(&'Л'));
        assert!(char_variants('A').is_empty());
    }
}
