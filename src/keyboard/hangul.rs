//! Hangul composition for the Korean keyboard (two-set, Dubeolsik): the
//! keys type jamo, which join the syllable before the caret as Korean input
//! methods do. ㅎ ㅏ ㄴ gives 한; another vowel moves the final consonant to
//! a new syllable (한 + ㅏ = 하나); compound vowels and finals join (ㅗ ㅏ =
//! ㅘ, ㄹ ㄱ = ㄺ). Backspace takes the last jamo off again.

use crate::alloc_prelude::*;

/// The initial consonants, in syllable order, as compatibility jamo.
const INITIALS: [char; 19] = ['ㄱ', 'ㄲ', 'ㄴ', 'ㄷ', 'ㄸ', 'ㄹ', 'ㅁ', 'ㅂ', 'ㅃ', 'ㅅ', 'ㅆ', 'ㅇ', 'ㅈ', 'ㅉ', 'ㅊ', 'ㅋ', 'ㅌ', 'ㅍ', 'ㅎ'];
/// The final consonants, in syllable order (after "none").
const FINALS: [char; 27] = [
    'ㄱ', 'ㄲ', 'ㄳ', 'ㄴ', 'ㄵ', 'ㄶ', 'ㄷ', 'ㄹ', 'ㄺ', 'ㄻ', 'ㄼ', 'ㄽ', 'ㄾ', 'ㄿ', 'ㅀ', 'ㅁ', 'ㅂ', 'ㅄ', 'ㅅ', 'ㅆ', 'ㅇ', 'ㅈ', 'ㅊ', 'ㅋ', 'ㅌ', 'ㅍ', 'ㅎ',
];
/// Two vowels that make one.
const COMPOUND_VOWELS: [(char, char, char); 7] =
    [('ㅗ', 'ㅏ', 'ㅘ'), ('ㅗ', 'ㅐ', 'ㅙ'), ('ㅗ', 'ㅣ', 'ㅚ'), ('ㅜ', 'ㅓ', 'ㅝ'), ('ㅜ', 'ㅔ', 'ㅞ'), ('ㅜ', 'ㅣ', 'ㅟ'), ('ㅡ', 'ㅣ', 'ㅢ')];
/// Two final consonants that make one.
const COMPOUND_FINALS: [(char, char, char); 11] = [
    ('ㄱ', 'ㅅ', 'ㄳ'),
    ('ㄴ', 'ㅈ', 'ㄵ'),
    ('ㄴ', 'ㅎ', 'ㄶ'),
    ('ㄹ', 'ㄱ', 'ㄺ'),
    ('ㄹ', 'ㅁ', 'ㄻ'),
    ('ㄹ', 'ㅂ', 'ㄼ'),
    ('ㄹ', 'ㅅ', 'ㄽ'),
    ('ㄹ', 'ㅌ', 'ㄾ'),
    ('ㄹ', 'ㅍ', 'ㄿ'),
    ('ㄹ', 'ㅎ', 'ㅀ'),
    ('ㅂ', 'ㅅ', 'ㅄ'),
];

const SYLLABLES: u32 = 0xAC00;
const VOWEL_JAMO: u32 = 0x314F; // ㅏ: the 21 vowels follow in syllable order.

/// A syllable's (initial, vowel, final) indices; final 0 is none.
fn split(c: char) -> Option<(u32, u32, u32)> {
    let index = (c as u32).checked_sub(SYLLABLES).filter(|&i| i < 11172)?;
    Some((index / 588, index % 588 / 28, index % 28))
}

fn syllable(initial: u32, vowel: u32, fin: u32) -> char {
    char::from_u32(SYLLABLES + (initial * 21 + vowel) * 28 + fin).unwrap_or('\u{fffd}')
}

fn vowel_index(c: char) -> Option<u32> {
    (c as u32).checked_sub(VOWEL_JAMO).filter(|&i| i < 21)
}

fn vowel(index: u32) -> char {
    char::from_u32(VOWEL_JAMO + index).unwrap_or('\u{fffd}')
}

fn initial_index(c: char) -> Option<u32> {
    INITIALS.iter().position(|&i| i == c).map(|i| i as u32)
}

/// The final index of consonant `c` (1-based), if it can end a syllable.
fn final_index(c: char) -> Option<u32> {
    FINALS.iter().position(|&f| f == c).map(|i| i as u32 + 1)
}

fn final_jamo(index: u32) -> char {
    FINALS[index as usize - 1]
}

fn join(table: &[(char, char, char)], a: char, b: char) -> Option<char> {
    table.iter().find(|&&(x, y, _)| x == a && y == b).map(|&(_, _, joined)| joined)
}

fn unjoin(table: &[(char, char, char)], joined: char) -> Option<(char, char)> {
    table.iter().find(|&&(_, _, j)| j == joined).map(|&(a, b, _)| (a, b))
}

/// Whether `c` is a jamo this composes (a compatibility consonant or vowel).
pub(crate) fn is_jamo(c: char) -> bool {
    initial_index(c).is_some() || final_index(c).is_some() || vowel_index(c).is_some()
}

/// What `prev`, the character before the caret, becomes when `jamo` is
/// typed after it, or `None` when the jamo just follows it.
pub(crate) fn type_jamo(prev: char, jamo: char) -> Option<String> {
    if let Some(v) = vowel_index(jamo) {
        if let Some((i, pv, f)) = split(prev) {
            if f == 0 {
                // 고 + ㅏ = 과
                let joined = vowel_index(join(&COMPOUND_VOWELS, vowel(pv), jamo)?)?;
                return Some(syllable(i, joined, 0).to_string());
            }
            // The final starts the next syllable: 한 + ㅏ = 하나, 닭 + ㅏ = 달가.
            let (keep, moved) = match unjoin(&COMPOUND_FINALS, final_jamo(f)) {
                Some((first, second)) => (final_index(first)?, second),
                None => (0, final_jamo(f)),
            };
            return Some([syllable(i, pv, keep), syllable(initial_index(moved)?, v, 0)].iter().collect());
        }
        if let Some(i) = initial_index(prev) {
            // ㅎ + ㅏ = 하
            return Some(syllable(i, v, 0).to_string());
        }
        // ㅗ + ㅏ = ㅘ
        return join(&COMPOUND_VOWELS, prev, jamo).map(String::from);
    }
    let (i, v, f) = split(prev)?;
    if f == 0 {
        // 하 + ㄴ = 한
        return Some(syllable(i, v, final_index(jamo)?).to_string());
    }
    // 달 + ㄱ = 닭
    let joined = final_index(join(&COMPOUND_FINALS, final_jamo(f), jamo)?)?;
    Some(syllable(i, v, joined).to_string())
}

/// What `prev` becomes when backspace takes its last jamo off, or `None`
/// when it goes entirely.
pub(crate) fn remove_jamo(prev: char) -> Option<char> {
    if let Some((i, v, f)) = split(prev) {
        if f != 0 {
            // 닭 → 달 → 다
            let fin = unjoin(&COMPOUND_FINALS, final_jamo(f)).and_then(|(first, _)| final_index(first)).unwrap_or(0);
            return Some(syllable(i, v, fin));
        }
        // 과 → 고 → ㄱ
        return Some(match unjoin(&COMPOUND_VOWELS, vowel(v)) {
            Some((first, _)) => syllable(i, vowel_index(first)?, 0),
            None => INITIALS[i as usize],
        });
    }
    unjoin(&COMPOUND_VOWELS, prev).map(|(first, _)| first)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Types `keys` from nothing, as the keyboard does.
    fn typed(keys: &str) -> String {
        let mut text = String::new();
        for jamo in keys.chars() {
            match text.chars().next_back().and_then(|prev| type_jamo(prev, jamo)) {
                Some(replacement) => {
                    text.pop();
                    text.push_str(&replacement);
                }
                None => text.push(jamo),
            }
        }
        text
    }

    #[test]
    fn jamo_make_syllables() {
        assert_eq!(typed("ㅎㅏㄴㄱㅜㄱㅇㅓ"), "한국어");
        assert_eq!(typed("ㅎㅏㄴㅏ"), "하나");
        assert_eq!(typed("ㄷㅏㄹㄱ"), "닭");
        assert_eq!(typed("ㄷㅏㄹㄱㅏ"), "달가");
        assert_eq!(typed("ㄱㅗㅏㅇ"), "광");
        assert_eq!(typed("ㅇㅢㅅㅏ"), "의사");
        assert_eq!(typed("ㄱㄱ"), "ㄱㄱ");
        assert_eq!(typed("ㅏㅏ"), "ㅏㅏ");
        assert_eq!(typed("ㅗㅏ"), "ㅘ");
        // ㄸ can't end a syllable.
        assert_eq!(typed("ㅏㄸ"), "ㅏㄸ");
        assert_eq!(typed("ㄷㅏㄸ"), "다ㄸ");
    }

    #[test]
    fn backspace_takes_off_the_last_jamo() {
        let mut steps = Vec::new();
        let mut c = Some('닭');
        while let Some(current) = c {
            steps.push(current);
            c = remove_jamo(current);
        }
        assert_eq!(steps, ['닭', '달', '다', 'ㄷ']);
        assert_eq!(remove_jamo('과'), Some('고'));
        assert_eq!(remove_jamo('ㅘ'), Some('ㅗ'));
        assert_eq!(remove_jamo('ㄱ'), None);
        assert_eq!(remove_jamo('a'), None);
    }
}
