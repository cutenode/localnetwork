//! Properties of Camelot keys, tempo gaps and set ordering (`harmony`).

use hegel::generators as gs;
use hegel::{Generator, TestCase};
use locallibrary::Track;
use locallibrary::harmony::{self, Camelot, Relation};
use locallibrary::progress::Ctx;
use rustc_hash::FxHashMap;

fn all_keys() -> Vec<Camelot> {
    (1..=12).flat_map(|number| [false, true].map(|major| Camelot { number, major })).collect()
}

fn key(tc: &TestCase) -> Camelot {
    let (number, major) = tc.draw(gs::tuples!(gs::integers::<u8>().min_value(1).max_value(12), gs::booleans()));
    Camelot { number, major }
}

fn track(id: u32, key: &str, bpm: f64) -> Track {
    Track {
        id,
        title: format!("T{id}"),
        artist: "A".into(),
        album: String::new(),
        album_artist: String::new(),
        genre: String::new(),
        label: String::new(),
        key: key.into(),
        remixer: String::new(),
        original_artist: String::new(),
        composer: String::new(),
        comment: String::new(),
        mix_name: String::new(),
        isrc: String::new(),
        bpm,
        duration_secs: 300,
        year: 0,
        track_number: 0,
        disc_number: 0,
        rating: 0,
        color: None,
        play_count: 0,
        bitrate: 320,
        sample_rate: 44100,
        sample_depth: 16,
        file_size: 1,
        file_path: format!("/Contents/{id}.mp3"),
        analyze_path: String::new(),
        date_added: String::new(),
        release_date: String::new(),
        artwork_id: None,
        artwork_path: None,
    }
}

/// Every key reads back from its Camelot code, its Open Key-free note name, and in lower case.
#[hegel::test(test_cases = 200)]
fn keys_round_trip_through_every_notation(tc: TestCase) {
    let c = key(&tc);
    assert_eq!(Camelot::parse(&c.code()), Some(c));
    assert_eq!(Camelot::parse(&c.note()), Some(c), "{}", c.note());
    assert_eq!(Camelot::parse(&format!("  {}  ", c.code().to_lowercase())), Some(c));
    assert_eq!(harmony::key_order(&c.code()), c.order());
}

/// Parsing any text never panics, and whatever it accepts is a real key.
#[hegel::test(test_cases = 1000)]
fn parse_accepts_only_real_keys(tc: TestCase) {
    let s = tc.draw(hegel::one_of!(
        gs::text().max_size(12),
        gs::text().alphabet("0123456789ABCDEFGabdmj#♯♭ inor").max_size(8),
    ));
    if let Some(c) = Camelot::parse(&s) {
        assert!((1..=12).contains(&c.number), "{s:?} -> {c:?}");
        assert_eq!(Camelot::parse(&c.code()), Some(c));
    }
}

/// Wheel order is a bijection from the 24 keys onto 0..24.
#[test]
fn order_is_a_bijection() {
    let mut orders: Vec<u8> = all_keys().into_iter().map(Camelot::order).collect();
    orders.sort_unstable();
    assert_eq!(orders, (0..24).collect::<Vec<u8>>());
}

/// Mixing compatibility is symmetric, every key is the same as itself, and counting energy
/// boosts only ever adds relations.
#[hegel::test(test_cases = 500)]
fn relation_is_symmetric(tc: TestCase) {
    let (a, b) = (key(&tc), key(&tc));
    assert_eq!(a.relation(b), b.relation(a));
    assert_eq!(a.relation(a), Some(Relation::Same));
    if let Some(r) = a.relation(b) {
        assert_eq!(a.relation_with_boost(b), Some(r));
    }
    // A boost up the wheel is a move down from the other side, never a plain relation.
    if a.relation_with_boost(b) == Some(Relation::EnergyBoost) {
        assert_eq!(a.relation(b), None);
    }
}

/// A tempo is no distance from itself, half or double itself, and never negative.
#[hegel::test(test_cases = 500)]
fn tempo_gap_allows_half_and_double_time(tc: TestCase) {
    let a = tc.draw(gs::floats::<f64>().min_value(20.0).max_value(300.0));
    let b = tc.draw(gs::floats::<f64>().allow_nan(true));
    assert_eq!(harmony::tempo_gap(a, a), 0.0);
    assert!(harmony::tempo_gap(a, 2.0 * a) < 1e-12);
    assert!(harmony::tempo_gap(a, a / 2.0) < 1e-12);
    let g = harmony::tempo_gap(a, b);
    assert!(g >= 0.0 || g.is_nan() && b.is_nan(), "{g}");
}

/// Ordering a set gives back each of its tracks exactly once, starting with the requested track
/// (or else the calmest, slowest one), whatever the tempos and energies, NaN included.
#[hegel::test(test_cases = 300)]
#[hegel::explicit_test_case(
    n = 3usize,
    keys = vec![String::new(); 3],
    bpms = vec![f64::NAN, 120.0, 121.0],
    energies = vec![None::<f64>; 3],
    start = None::<usize>
)]
#[hegel::explicit_test_case(
    n = 2usize,
    keys = vec![String::new(); 2],
    bpms = vec![128.0, 120.0],
    energies = vec![None::<f64>; 2],
    start = None::<usize>
)]
#[hegel::explicit_test_case(
    n = 2usize,
    keys = vec![String::new(); 2],
    bpms = vec![120.0, 121.0],
    energies = vec![None::<f64>; 2],
    start = Some(1usize)
)]
fn order_set_is_a_permutation_starting_where_asked(tc: TestCase) {
    let n = tc.draw(gs::integers::<usize>().max_value(9));
    let keys = tc.draw(
        gs::vecs(hegel::one_of!(
            gs::sampled_from(vec!["8A", "9A", "8B", "Am", "1B", "12A"]).map(String::from),
            gs::text().max_size(3)
        ))
        .min_size(n)
        .max_size(n),
    );
    let bpms = tc.draw(gs::vecs(gs::floats::<f64>().allow_nan(true)).min_size(n).max_size(n));
    let energies = tc.draw(gs::vecs(gs::optional(gs::floats::<f64>().allow_nan(true))).min_size(n).max_size(n));
    let start = tc.draw(gs::optional(gs::integers::<usize>().max_value(n)));
    let n = keys.len();
    let tracks: Vec<Track> = (0..n).map(|i| track(i as u32, &keys[i], bpms[i])).collect();
    let energy: FxHashMap<u32, f64> =
        energies.iter().enumerate().filter_map(|(i, e)| e.map(|e| (i as u32, e))).collect();
    let refs: Vec<&Track> = tracks.iter().collect();
    let start = start.map(|s| s as u32);

    let order = harmony::order_set(&refs, start, &energy, &FxHashMap::default(), &Ctx::silent()).unwrap();
    let mut sorted = order.clone();
    sorted.sort_unstable();
    assert_eq!(sorted, (0..n as u32).collect::<Vec<_>>(), "a permutation");
    match start {
        Some(s) if (s as usize) < n => assert_eq!(order[0], s),
        _ if n > 0 => {
            // The calmest, slowest track opens: nothing sorts before it.
            let e = |id: u32| energy.get(&id).copied().unwrap_or(2.0);
            let first = (e(order[0]), tracks[order[0] as usize].bpm);
            for t in &tracks {
                let other = (e(t.id), t.bpm);
                assert!(first.0.total_cmp(&other.0).then(first.1.total_cmp(&other.1)).is_le(), "{first:?} {other:?}");
            }
        }
        _ => {}
    }
}
