//! Fixes for mod scripts that cannot show what they were written to show.
//!
//! Each fix is recognised by the script's own text, never by a file or bus name, and is
//! applied to the source lines before they are compiled.

/// The 4-character Annax line matrix of the LiAZ 5292 (a rewrite of the stock 3-character
/// `Matrix_D.osc`): a line number of one digit is first padded to four characters
/// (`4 $SetLengthL` → `"5   "`) and then cut to its last three (`3 $SetLengthR`, which
/// keeps the right end in OMSI too - see FORMATS.md), so line 5 came out blank and 5E as
/// `"   E"`. The number is written with three digits instead (`005E`, `051E`, `123E`), which
/// is what a three-digits-and-a-letter display shows.
///
/// The display has three cells for digits and a fourth for the letter, but the script
/// writes the letter lines Berlin style in front of the last digit or two (`" D" … 1
/// $SetLengthR " " $+ $+`): line 52D came out as `" D2 "`. Those lines are rewritten to
/// the number's three digits followed by the letter (`052D`), as the lines with a letter
/// behind the number (`10`, `30`…) already were.
fn four_char_matrix(lines: &mut [String]) -> bool {
    let has = |s: &str| {
        lines
            .iter()
            .any(|l| l.split_whitespace().collect::<Vec<_>>().join(" ") == s)
    };
    if !(has("(L.$.Matrix_NewNr) $length 1 <=")
        && has("(L.$.Matrix_NewNr) 3 $SetLengthR \"E\" $+")
        && has("4 $SetLengthL"))
    {
        return false;
    }
    let mut changed = false;
    // whatever the letter code made of it, the number stored for the display goes through
    // one rule at the end: digits first, the letter in the fourth cell
    let n = lines.len();
    for i in 0..n {
        if lines[i].split_whitespace().collect::<Vec<_>>().join(" ") != "4 $SetLengthR" {
            continue;
        }
        let next = lines[i + 1..]
            .iter()
            .map(|l| l.trim())
            .find(|l| !l.is_empty());
        if next == Some("(S.$.Matrix_NewNr)") {
            let indent: String = lines[i].chars().take_while(|c| c.is_whitespace()).collect();
            lines[i] = format!("{indent}4 $SetLengthR $__DigitsFirst");
            changed = true;
        }
    }
    for l in lines.iter_mut() {
        if l.split_whitespace().collect::<Vec<_>>().join(" ") == "l1 trunc $IntToStr" {
            let indent: String = l.chars().take_while(|c| c.is_whitespace()).collect();
            *l = format!("{indent}l1 trunc \"03\" $IntToStrEnh");
            changed = true;
        } else if let Some(letter) = letter_in_front(l) {
            let indent: String = l.chars().take_while(|c| c.is_whitespace()).collect();
            *l = format!("{indent}(L.$.Matrix_NewNr) 3 $SetLengthR \"{letter}\" $+");
            changed = true;
        }
    }
    changed
}

/// `"E" (L.$.Matrix_NewNr) 2 $SetLengthR " " $+ $+` or `" D" (L.$.Matrix_NewNr) 1
/// $SetLengthR " " $+ $+`: the letter written in front of the number.
fn letter_in_front(line: &str) -> Option<char> {
    let t = line.trim();
    let rest = t.strip_prefix('"')?;
    let (prefix, rest) = rest.split_once('"')?;
    let words: Vec<&str> = rest.split_whitespace().collect();
    if words.len() != 7
        || words[0] != "(L.$.Matrix_NewNr)"
        || !matches!(words[1], "1" | "2")
        || words[2] != "$SetLengthR"
        || words[3] != "\""
        || words[4] != "\""
        || words[5] != "$+"
        || words[6] != "$+"
    {
        return None;
    }
    let mut letters = prefix.chars().filter(|c| !c.is_whitespace());
    let letter = letters.next().filter(|c| c.is_ascii_alphabetic())?;
    letters.next().is_none().then_some(letter)
}

/// Apply every fix that recognises `lines`; returns the names of those applied.
pub fn patch(lines: &mut [String]) -> Vec<&'static str> {
    let mut applied = Vec::new();
    if four_char_matrix(lines) {
        applied.push("4-character line matrix: number padded to three digits");
    }
    applied
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn liaz_matrix_pads_the_number() {
        let src = "\t\t\t\t\t\tl1 trunc $IntToStr\n(L.$.Matrix_NewNr) $length 1 <=\n4 $SetLengthL\n\t(L.$.Matrix_NewNr) 3 $SetLengthR \"E\" $+";
        let mut lines: Vec<String> = src.lines().map(String::from).collect();
        assert_eq!(patch(&mut lines).len(), 1);
        assert_eq!(lines[0], "\t\t\t\t\t\tl1 trunc \"03\" $IntToStrEnh");
        // the stock 3-character matrix is left alone
        let mut stock: Vec<String> = "l1 trunc $IntToStr\n(L.$.Matrix_NewNr) $length 1 <=\n2 $SetLengthL\n\"E\" (L.$.Matrix_NewNr) 2 $SetLengthR $+".lines().map(String::from).collect();
        assert!(patch(&mut stock).is_empty());
    }

    #[test]
    fn liaz_matrix_letter_behind_the_digits() {
        let src = "l1 trunc $IntToStr\n(L.$.Matrix_NewNr) $length 1 <=\n4 $SetLengthL\n(L.$.Matrix_NewNr) 3 $SetLengthR \"E\" $+\n\t\t\" D\" (L.$.Matrix_NewNr) 1 $SetLengthR \" \" $+ $+\n\"E\" (L.$.Matrix_NewNr) 2 $SetLengthR \" \" $+ $+\n\"BVG \"";
        let mut lines: Vec<String> = src.lines().map(String::from).collect();
        patch(&mut lines);
        assert_eq!(lines[4], "\t\t(L.$.Matrix_NewNr) 3 $SetLengthR \"D\" $+");
        assert_eq!(lines[5], "(L.$.Matrix_NewNr) 3 $SetLengthR \"E\" $+");
        assert_eq!(lines[6], "\"BVG \"");
    }
}
