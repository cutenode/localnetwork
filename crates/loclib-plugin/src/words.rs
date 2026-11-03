//! Words around the names manifests give things, which start with any letter.

/// `word` after "a", or after "an" when it starts with a vowel letter: "an archive account", "a
/// plyr account", "an integer".
pub fn a_or_an(word: &str) -> String {
    let vowel = word.starts_with(|c: char| "aeiouAEIOU".contains(c));
    format!("{} {word}", if vowel { "an" } else { "a" })
}

#[cfg(test)]
mod tests;
