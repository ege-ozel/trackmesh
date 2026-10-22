//! The Desktop window's language, also used by its native dialogs and the DAW plugin.
use std::sync::atomic::{AtomicBool, Ordering};

static TURKISH: AtomicBool = AtomicBool::new(false);

pub fn is_turkish() -> bool {
    TURKISH.load(Ordering::Relaxed)
}

pub fn set_turkish(turkish: bool) {
    TURKISH.store(turkish, Ordering::Relaxed);
}

pub fn tr<'a>(en: &'a str, turkish: &'a str) -> &'a str {
    if is_turkish() {
        turkish
    } else {
        en
    }
}

/// How a name ends when read aloud, as the window's `spoken`: a last word of one or two capitals or a
/// digit is spelled out ("Studio B" → "be", "PC" → "ce"), so the suffix reads Studio B'nin, not B'un.
fn spoken(name: &str) -> String {
    const LETTERS: &str = "ABCÇDEFGĞHIİJKLMNOÖPQRSŞTUÜVWXYZ0123456789";
    const SAID: [&str; 42] = [
        "a", "be", "ce", "çe", "de", "e", "fe", "ge", "ge", "he", "ı", "i", "je", "ke", "le", "me",
        "ne", "o", "ö", "pe", "ku", "re", "se", "şe", "te", "u", "ü", "ve", "ve", "iks", "ye",
        "ze", "sıfır", "bir", "iki", "üç", "dört", "beş", "altı", "yedi", "sekiz", "dokuz",
    ];
    let trimmed = name.trim();
    let word = trimmed.split_whitespace().last().unwrap_or("");
    let capitals =
        (1..=2).contains(&word.chars().count()) && word.chars().all(|ch| ch.is_uppercase());
    if let Some(end) = trimmed
        .chars()
        .last()
        .filter(|end| capitals || end.is_ascii_digit())
    {
        if let Some(index) = LETTERS.chars().position(|letter| letter == end) {
            return SAID[index].into();
        }
    }
    name.to_lowercase()
        .replace('â', "a")
        .replace('î', "i")
        .replace('û', "u")
}

/// A name with the Turkish genitive suffix by vowel harmony (Ege'nin, Burak'ın), as the window's `named(name, 'gen')`.
pub fn genitive(name: &str) -> String {
    let lower = spoken(name);
    let is_vowel = |ch: char| "aeıioöuü".contains(ch);
    let last = lower.chars().rev().find(|ch| is_vowel(*ch)).unwrap_or('e');
    let ends_vowel = lower.chars().last().is_some_and(is_vowel);
    let four = match last {
        'a' | 'ı' => 'ı',
        'e' | 'i' => 'i',
        'o' | 'u' => 'u',
        _ => 'ü',
    };
    format!("{name}'{}{four}n", if ends_vowel { "n" } else { "" })
}
