use super::*;

fn args(a: &[&str]) -> Vec<String> {
    a.iter().map(|s| s.to_string()).collect()
}

#[test]
fn plist_runs_watch_and_escapes_paths() {
    let p = plist(
        Path::new("/Users/r&b/.cargo/bin/locallibrary"),
        &args(&["--store", "/Volumes/<NAS>", "watch", "--pull"]),
        Path::new("/Users/r&b/Library/Logs/locallibrary-watch.log"),
    );
    assert!(p.contains("<string>local.locallibrary.watch</string>"));
    assert!(p.contains(
        "    <string>/Users/r&amp;b/.cargo/bin/locallibrary</string>\n    <string>--store</string>\n    \
             <string>/Volumes/&lt;NAS&gt;</string>\n    <string>watch</string>\n    <string>--pull</string>\n  </array>"
    ));
    assert!(p.contains("<key>StandardErrorPath</key>\n  <string>/Users/r&amp;b/Library/Logs/locallibrary-watch.log"));
}

#[test]
fn unit_quotes_each_argument() {
    let u = unit(
        Path::new("/home/dj/.cargo/bin/locallibrary"),
        &args(&["--store", "/mnt/100% \"mine\" $HOME", "watch", "--pull"]),
    );
    assert!(u.contains(
        r#"ExecStart="/home/dj/.cargo/bin/locallibrary" "--store" "/mnt/100%% \"mine\" $$HOME" "watch" "--pull""#
    ));
    assert!(u.contains("WantedBy=default.target"));
}
