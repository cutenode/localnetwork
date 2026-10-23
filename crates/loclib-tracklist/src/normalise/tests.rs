use super::*;

#[test]
fn titles_and_artists_normalise() {
    let k = Key::new("Get Lucky (feat. Pharrell Williams) [Radio Edit]", "Daft Punk", "", None);
    assert_eq!(k.title, "get lucky");
    assert_eq!(k.artists, ["daft punk", "pharrell williams"]);
    assert!(k.mix.is_empty());
    assert_eq!(k.variant, ["edit", "radio"]);
    let k = Key::new("Levels - Skrillex Remix", "Avicii", "", None);
    assert_eq!((k.title.as_str(), k.mix.clone()), ("levels", vec!["remix".to_string(), "skrillex".into()]));
    let k = Key::new("(I Can't Get No) Satisfaction - Mono Version", "The Rolling Stones", "", None);
    assert_eq!(
        (k.title.as_str(), k.artists.clone()),
        ("i cant get no satisfaction", vec!["rolling stones".to_string()])
    );
    let k = Key::new("Song Title feat. Somebody", "Árvore & Co.", "Original Mix", None);
    assert_eq!(
        (k.title.as_str(), k.artists.clone()),
        ("song title", vec!["arvore".to_string(), "co".into(), "somebody".into()])
    );
    assert!(k.mix.is_empty() && k.variant.is_empty());
    let k = Key::new("Track [Anjunadeep]", "A", "", None);
    assert_eq!((k.title.as_str(), k.variant.clone()), ("track", vec!["anjunadeep".to_string()]));
    // A title that is nothing but brackets keeps its words.
    assert_eq!(Key::new("(Untitled)", "", "", None).title, "untitled");
    let k = Key::new("Dancing - With Myself (feat. Someone) (with Other)", "Billy Idol", "", None);
    assert_eq!((k.title.as_str(), k.artists.len()), ("dancing with myself", 3));
    assert_eq!(split_artists("Simon and Garfunkel"), split_artists("Simon & Garfunkel"));
    assert_eq!(split_artists("Disclosure x Sam Smith"), ["disclosure", "sam smith"]);
    assert_eq!(split_artists("Malcolm X"), ["malcolm x"]);
    assert!(near_words("arround", "around") && near_words("colour", "color"));
    assert!(!near_words("2011", "2012") && !near_words("tone", "tune") && !near_words("strobe", "probes"));
}

use hegel::generators as gs;
use hegel::{Generator, TestCase};

/// Words that mean something to the parser: version, stop and credit words, and the
/// separators between artists.
fn reserved(w: &str) -> bool {
    MIX_WORDS.contains(&w)
        || GENERIC.contains(&w)
        || FEAT_WORDS.contains(&w)
        || ["with", "vs", "x", "presents", "pres", "meets", "rmx", "remastered", "acappella", "accapella"].contains(&w)
}

/// 1 to `max` plain lower-case words the parser gives no meaning to.
#[hegel::test_helper]
fn plain_words(tc: &TestCase, max: usize) -> Vec<String> {
    let word = gs::from_regex("[a-z]{2,8}").fullmatch(true).filter(|w: &String| !reserved(w));
    tc.draw(gs::vecs(word).min_size(1).max_size(max))
}

/// Folding ignores case, accents (composed or not), apostrophes, dots, and how words are
/// spaced or punctuated apart; "&" is "and"; and folding twice changes nothing.
#[hegel::test(test_cases = 300)]
fn folding_ignores_spelling(tc: TestCase) {
    let words = plain_words(&tc, 5);
    let plain = words.join(" ");
    let mut messy = String::new();
    for (k, w) in words.iter().enumerate() {
        if k > 0 {
            messy.push_str(
                tc.draw(gs::sampled_from(vec![" ", "  ", "\t", "\u{a0}", "\u{3000}", " - ", ", ", "/", " (", ") "])),
            );
        }
        for c in w.chars() {
            messy.push(if tc.draw(gs::booleans()) { c.to_ascii_uppercase() } else { c });
            if "aeiounc".contains(c) && tc.draw(gs::booleans()) {
                messy.push(tc.draw(gs::sampled_from(vec!['\u{301}', '\u{300}', '\u{308}', '\u{303}', '\u{327}'])));
            }
            if tc.draw(gs::weighted_booleans(0.1)) {
                messy.push(tc.draw(gs::sampled_from(vec!['\'', '\u{2019}', '`', '.'])));
            }
        }
    }
    let messy = if tc.draw(gs::booleans()) { messy.nfc().collect() } else { messy };
    tc.note(&messy);
    assert_eq!(fold(&messy), plain);
    assert_eq!(fold(&format!("{} & {}", words[0], plain)), format!("{} and {plain}", words[0]));
    let any = tc.draw(gs::text().max_size(20));
    assert_eq!(fold(&fold(&any)), fold(&any));
}

/// A version written in brackets, after a dash or as the mix name is the same version: the
/// title is the same as without it, and a remix's distinct words are its remixer and
/// "remix". Generic versions ("Original Mix", a year's remaster) make no different recording.
#[hegel::test(test_cases = 300)]
fn versions_split_off_the_title(tc: TestCase) {
    let title = plain_words(&tc, 3).join(" ");
    let remixer = plain_words(&tc, 1).remove(0);
    let bare = Key::new(&title, "Someone", "", None);
    assert_eq!(bare.title, title);
    assert!(bare.mix.is_empty() && bare.variant.is_empty());
    let remix = format!("{remixer} Remix");
    let (t, mix) = match tc.draw(gs::integers::<u8>().max_value(4)) {
        0 => (format!("{title} ({remix})"), ""),
        1 => (format!("{title} [{remix}]"), ""),
        2 => (format!("{title} - {remix}"), ""),
        3 => (format!("{title} \u{2013} {remix}"), ""),
        _ => (title.clone(), remix.as_str()),
    };
    let k = Key::new(&t, "Someone", mix, None);
    let mut want = vec![remixer, "remix".to_string()];
    want.sort();
    assert_eq!((k.title.as_str(), &k.mix), (title.as_str(), &want), "{t:?} {mix:?}");
    let generic = tc.draw(gs::sampled_from(vec![
        " (Original Mix)",
        " [Extended Mix]",
        " - Radio Edit",
        " (2012 Remaster)",
        " - Remastered 2009",
    ]));
    let k = Key::new(&format!("{title}{generic}"), "Someone", "", None);
    assert_eq!(k.title, title);
    assert!(k.mix.is_empty(), "{generic}: {:?}", k.mix);
    // A trailing aside that names no version ("[Anjunadeep]") only tells versions apart.
    let aside = plain_words(&tc, 2).join(" ");
    let k = Key::new(&format!("{title} [{aside}]"), "Someone", "", None);
    let mut want: Vec<String> = aside.split(' ').map(String::from).collect();
    want.sort();
    want.dedup();
    assert_eq!((k.title.as_str(), k.mix.len(), &k.variant), (title.as_str(), 0, &want));
    // Brackets with more of the name after them are part of it ("(I Can't Get No) Satisfaction").
    assert_eq!(Key::new(&format!("({title}) {aside}"), "Someone", "", None).title, format!("{title} {aside}"));
}

/// A guest is credited the same whether the title says it (in brackets, after a dash or
/// plainly) or the artist field does: the title is the song's, the artists both.
#[hegel::test(test_cases = 300)]
fn featured_artists_join_the_artists(tc: TestCase) {
    let title = plain_words(&tc, 3).join(" ");
    let (artist, guest) = (plain_words(&tc, 2).join(" "), plain_words(&tc, 2).join(" "));
    tc.assume(artist != guest);
    let (t, a) = match tc.draw(gs::integers::<u8>().max_value(8)) {
        0 => (format!("{title} (feat. {guest})"), artist.clone()),
        1 => (format!("{title} [ft. {guest}]"), artist.clone()),
        2 => (format!("{title} (with {guest})"), artist.clone()),
        3 => (format!("{title} - feat. {guest}"), artist.clone()),
        4 => (format!("{title} featuring {guest}"), artist.clone()),
        5 => (title.clone(), format!("{artist} feat. {guest}")),
        6 => (title.clone(), format!("{artist}, {guest}")),
        7 => (title.clone(), format!("{artist} & {guest}")),
        _ => (title.clone(), format!("{artist} x {guest}")),
    };
    let k = Key::new(&t, &a, "", None);
    assert_eq!((k.title, k.artists), (title, vec![artist, guest]), "{t:?} by {a:?}");
}

/// Artist credits split into their names at every separator credit lists use, each folded
/// and without a leading "the", once each.
#[hegel::test(test_cases = 300)]
fn artist_credits_split_into_names(tc: TestCase) {
    let names: Vec<String> = (0..tc.draw(gs::integers::<usize>().min_value(1).max_value(4)))
        .map(|_| plain_words(&tc, 2).join(" "))
        .collect();
    let mut credit = String::new();
    for (k, n) in names.iter().enumerate() {
        if k > 0 {
            credit.push_str(tc.draw(gs::sampled_from(vec![
                ", ",
                "; ",
                " & ",
                " and ",
                " feat. ",
                " ft. ",
                " vs. ",
                " x ",
                "/",
                " + ",
                " with ",
                " presents ",
                " meets ",
            ])));
        }
        let the = if tc.draw(gs::booleans()) { "The " } else { "" };
        credit.push_str(&format!("{the}{}", n.to_uppercase()));
    }
    let mut want: Vec<String> = Vec::new();
    for n in names {
        if !want.contains(&n) {
            want.push(n);
        }
    }
    assert_eq!(split_artists(&credit), want, "{credit:?}");
}

/// Words of five or more letters are near when one letter is changed (two in words of nine
/// or more), not when two are in a shorter word, never when a digit differs, and nearness
/// doesn't depend on the order of the two.
#[hegel::test(test_cases = 300)]
fn near_words_allow_one_typo(tc: TestCase) {
    let w: Vec<char> = tc.draw(gs::from_regex("[a-y]{5,12}").fullmatch(true)).chars().collect();
    let changed = |at: &[usize], to: char| -> String {
        w.iter().enumerate().map(|(i, &c)| if at.contains(&i) { to } else { c }).collect()
    };
    let word: String = w.iter().collect();
    let i = tc.draw(gs::integers::<usize>().max_value(w.len() - 1));
    let j = tc.draw(gs::integers::<usize>().max_value(w.len() - 1));
    assert!(near_words(&word, &word));
    let one = changed(&[i], 'z');
    assert!(near_words(&word, &one) && near_words(&one, &word));
    if i != j {
        let two = changed(&[i, j], 'z');
        assert_eq!(near_words(&word, &two), w.len() >= 9);
        assert_eq!(near_words(&two, &word), w.len() >= 9);
    }
    assert!(!near_words(&word, &changed(&[i], '7')));
}

/// Scores stay between 0 and 1, and anything with a title and an artist scores 1 against
/// itself.
#[hegel::test(test_cases = 300)]
fn scores_peak_on_identity(tc: TestCase) {
    let key = |tc: &TestCase| {
        let field = || gs::text().max_size(16);
        let duration = tc.draw(gs::optional(gs::integers::<u32>().max_value(900)));
        Key::new(&tc.draw(field()), &tc.draw(field()), &tc.draw(field()), duration)
    };
    let (a, b) = (key(&tc), key(&tc));
    for s in [score(&a, &b), score(&b, &a), score(&a, &a)] {
        assert!((0.0..=1.0).contains(&s), "{s}");
    }
    if !a.title_tokens.is_empty() && !a.artist_tokens.is_empty() {
        assert!(score(&a, &a) > 0.999, "{a:?}");
    }
}

/// A version word's spelling variants are one token: "rmx" is "remix", "acappella" and
/// "accapella" are "acapella" and "remastered" is "remaster", in any case and among any other
/// words, which stay as they are.
#[hegel::test(test_cases = 200)]
fn version_word_spellings_are_one_token(tc: TestCase) {
    let variants = [("rmx", "remix"), ("acappella", "acapella"), ("accapella", "acapella"), ("remastered", "remaster")];
    let mut words = plain_words(&tc, 4);
    let at = tc.draw(gs::integers::<usize>().max_value(words.len()));
    let (variant, word) = tc.draw(gs::sampled_from(&variants));
    let spelled: String =
        variant.chars().map(|c| if tc.draw(gs::booleans()) { c.to_ascii_uppercase() } else { c }).collect();
    let mut with_word = words.clone();
    with_word.insert(at, word.to_string());
    words.insert(at, spelled);
    assert_eq!(tokens(&words.join(" ")), with_word);
}
