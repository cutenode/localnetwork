use super::*;
use loclib_model::test_util::track;

/// Tracks found through one map are the ones finding each in the library finds, a repeated id
/// its first track, and an id no track has fails it, naming the first such.
#[hegel::test(test_cases = 200)]
fn tracks_named_are_those_the_library_finds(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let tracks: Vec<u32> = tc.draw(gs::vecs(gs::integers::<u32>().min_value(1).max_value(6)).max_size(8));
    let library = Library {
        tracks: tracks
            .iter()
            .enumerate()
            .map(|(i, &id)| track(id, "A", &i.to_string(), &format!("/{i}.mp3")))
            .collect(),
        ..Library::default()
    };
    let ids: Vec<u32> = tc.draw(gs::vecs(gs::integers::<u32>().min_value(1).max_value(7)).max_size(6));
    let each: Result<Vec<&Track>> =
        ids.iter().map(|&id| library.track(id).ok_or_else(|| Error::no_track(id))).collect();
    let text = |r: Result<Vec<&Track>>| {
        r.map(|t| t.iter().map(|t| t.title.clone()).collect::<Vec<_>>()).map_err(|e| e.to_string())
    };
    assert_eq!(text(tracks_named(&library, &ids)), text(each));
}
