//! Requests' scratch folders: where a plugin leaves files its answer names (an image for an
//! artwork edit), which the host reads itself, refusing a name that leads out of the folder and a
//! folder holding too much; and a view's rows sent in parts.

use locallibrary::edit::EditPlan;
use locallibrary::op::Planned;

use super::*;

/// The echo plugin, which may plan artwork.
fn artist(options: Options) -> Fixture {
    Fixture::new(|dir| vec![spec(dir, &granting(&["artwork:plan"]), &[])], options)
}

impl Fixture {
    /// Its operation answering `edits`, with the files `blobs` (by name) in its scratch folder
    /// and the links `links` there.
    fn plan_with(&self, edits: Value, blobs: &[(&str, &[u8])], links: Value) -> Result<Planned<EditPlan>> {
        let blobs: serde_json::Map<String, Value> =
            blobs.iter().map(|(name, bytes)| (name.to_string(), json!(bytes))).collect();
        let params = json!({ "name": "x", "edits": edits, "blobs": blobs, "links": links });
        self.plugins.plan_op("echo", "playlist", params, self.context(), Some(&self.store), &Ctx::silent())?.plan()
    }
}

/// An artwork edit with the image `name` in the scratch folder.
fn artwork(name: &str) -> Value {
    json!([{ "op": "set_artwork", "tracks": { "ids": [1] }, "image": { "blob": name } }])
}

/// Each request has a scratch folder of its own, in the plugin's folder beside where it runs,
/// there while it runs and gone once it has answered; a plugin without the `blobs` feature has
/// none.
#[test]
fn each_request_has_a_scratch_folder_while_it_runs() {
    let f = Fixture::echo();
    let context = |f: &Fixture, store: Option<&Store>| {
        let params = json!({ "context": true });
        match f.plugins.run_command("echo", "echo", params, f.context(), store, &Ctx::silent(), &|_| {}).unwrap() {
            CommandOutput::Headless(v) => v,
            view => panic!("{view:?}"),
        }
    };
    let [one, two] = [(); 2].map(|()| PathBuf::from(context(&f, Some(&f.store))["scratch"].as_str().unwrap()));
    assert_ne!(one, two, "one each");
    assert_eq!(one.parent().unwrap(), f.store.root().join("plugins/echo/scratch"));
    assert!(!one.exists() && !two.exists(), "gone once answered");
    // Without a store, in the plugin's own temporary folder.
    f.plugins.shutdown();
    let none = PathBuf::from(context(&f, None)["scratch"].as_str().unwrap());
    assert!(none.starts_with(std::env::temp_dir().canonicalize().unwrap()) || none.starts_with(std::env::temp_dir()));
    assert_eq!(none.parent().unwrap().file_name().unwrap(), "scratch");

    let f = Fixture::new(|dir| vec![spec(dir, ECHO, &["--without", "blobs"])], Options::default());
    assert!(context(&f, Some(&f.store)).get("scratch").is_none());
}

/// An artwork edit gives its image as a file in the scratch folder, which the host reads as it
/// plans: the plan holds it, and applies it once the folder is gone.
#[test]
fn an_image_is_a_file_in_the_scratch_folder() {
    let f = artist(Options::default());
    let planned = f.plan_with(artwork("covers/1.jpg"), &[], json!({})).map_err(failure).unwrap_err();
    assert!(
        planned.starts_with("the Echo plugin failed: its edit 0: its file \"covers/1.jpg\" isn't there"),
        "{planned}"
    );
    let planned = f.plan_with(artwork("cover.jpg"), &[("cover.jpg", &jpeg(300))], json!({})).unwrap();
    assert_eq!(planned.summary[1], "set the artwork of 1 track(s)", "{:?}", planned.summary);
    let before = common::library(&f.drive).track(1).unwrap().artwork_path.clone();
    planned.apply(None, &Ctx::silent()).unwrap();
    let after = common::library(&f.drive).track(1).unwrap().artwork_path.clone();
    assert!(after.is_some() && after != before, "{before:?} {after:?}");
}

/// What a plugin may not give: an image in base64 or by path, or a file leading out of the
/// folder, through a name or a link; and more than the folder may hold, which fails its answer.
#[test]
fn an_image_leading_out_of_the_scratch_folder_is_refused() {
    let f = artist(Options { max_blobs: 100, ..Options::default() });
    let given = |image: Value| {
        let edits = json!([{ "op": "set_artwork", "tracks": { "ids": [1] }, "image": image }]);
        failure(f.plan_with(edits, &[], json!({})).unwrap_err())
    };
    let otherwise = "the Echo plugin failed: its edit 0 gives an image otherwise than as a file in its scratch folder ({\"blob\": name})";
    assert_eq!(given(json!({ "base64": "AAAA" })), otherwise);
    assert_eq!(given(json!({ "path": "/tmp/cover.jpg" })), otherwise);
    for name in ["../cover.jpg", "/tmp/cover.jpg", ""] {
        let e = given(json!({ "blob": name }));
        assert_eq!(
            e,
            format!("the Echo plugin failed: its edit 0: {name:?} isn't a file's name in the scratch folder")
        );
    }
    #[cfg(unix)]
    {
        let outside = f.drive.parent().unwrap().join("outside.jpg");
        std::fs::write(&outside, [0; 10]).unwrap();
        let e = failure(f.plan_with(artwork("link.jpg"), &[], json!({ "link.jpg": outside })).unwrap_err());
        assert_eq!(e, "the Echo plugin failed: its edit 0: its file \"link.jpg\" leads out of the scratch folder");
    }
    let e = failure(f.plan_with(artwork("big.jpg"), &[("big.jpg", &[0; 101])], json!({})).unwrap_err());
    assert_eq!(e, "the Echo plugin failed: it left 101 B in its scratch folder, more than the 100 B a request may");
}

/// A view command's rows (or items) sent in parts are shown as they come, in order, before it
/// answers the view that has them all; a headless command's aren't, nor a plugin's without the
/// `blobs` feature.
#[test]
fn a_views_rows_sent_in_parts_are_shown_as_they_come() {
    let run = |args: &'static [&'static str], command: &str, params: Value| {
        let f = Fixture::new(move |dir| vec![spec(dir, ECHO, args)], Options::default());
        let shown = Mutex::new(Vec::new());
        let answer = f
            .plugins
            .run_command("echo", command, params, f.context(), None, &Ctx::silent(), &|p| shown.lock().unwrap().push(p))
            .unwrap();
        let shown: Vec<Value> = shown.into_inner().unwrap().into_iter().map(|p| json!([p.rows, p.items])).collect();
        (shown, answer)
    };
    let (shown, answer) = run(&[], "titles", json!({ "partial": ["a", "b"] }));
    assert_eq!(shown, [json!([[["a"]], []]), json!([[["b"]], []])]);
    let CommandOutput::View(View::Table { rows, .. }) = answer else { panic!("a table") };
    assert_eq!(rows, [[json!("a")], [json!("b")]], "the view has them all");
    let (shown, _) = run(&[], "titles", json!({ "partial_items": ["x"] }));
    assert_eq!(shown, [json!([[], [{ "label": "x" }]])]);
    assert!(run(&[], "echo", json!({ "partial": ["a"] })).0.is_empty(), "a headless command's aren't");
    assert!(run(&["--without", "blobs"], "titles", json!({ "partial": ["a"] })).0.is_empty());
}
