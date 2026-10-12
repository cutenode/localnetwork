use super::*;

#[test]
fn every_method_is_described_once_with_its_schemas() {
    let c = contract();
    let methods = c["methods"].as_array().unwrap();
    assert_eq!(methods.len(), registry().methods().count());
    let defs = c["$defs"].as_object().unwrap();
    // Two types with one name get a number from schemars: give one a `#[schemars(rename)]`.
    let numbered: Vec<&String> = defs.keys().filter(|k| k.ends_with(|c: char| c.is_ascii_digit())).collect();
    assert!(numbered.is_empty(), "types sharing a schema name: {numbered:?}");
    let mut missing = Vec::new();
    refs(c, &mut missing);
    missing.retain(|name| !defs.contains_key(name));
    assert!(missing.is_empty(), "undefined: {missing:?}");
    let sync = methods.iter().find(|m| m["name"] == "sync.apply").unwrap();
    assert_eq!(
        (&sync["kind"], &sync["pair"], &sync["operation"]),
        (&json!("apply"), &json!("sync.plan"), &json!("sync"))
    );
    let any_of = sync["params"]["anyOf"].as_array().unwrap();
    assert_eq!(any_of[0]["$ref"], "#/$defs/ApplyPlan");
    let many = methods.iter().find(|m| m["name"] == "sync.many.plan").unwrap();
    assert_eq!(many["aliases"], json!(["sync_many.plan"]));
    // A plan's result is the plan and what every plan says.
    let plan = &methods.iter().find(|m| m["name"] == "edit.plan").unwrap()["result"];
    assert_eq!(plan["allOf"][1]["$ref"], "#/$defs/PlanFields", "{plan}");
}

#[test]
fn one_method_comes_with_only_what_it_uses() {
    let one = describe(Some("drive.adopt")).unwrap();
    assert_eq!(one["methods"].as_array().unwrap().len(), 1);
    assert_eq!(one["methods"][0]["name"], "drive.adopt.apply");
    let defs = one["$defs"].as_object().unwrap();
    assert!(defs.contains_key("AdoptDrive") && !defs.contains_key("MixReport"), "{:?}", defs.keys());
    assert!(describe(Some("no.such")).is_none() && describe(Some("$/test.wait")).is_none());
}

/// One method described, by its name or an old one, is the contract's entry for it, with
/// exactly the definitions it uses, each as the contract has it; a name no method has
/// describes nothing.
#[hegel::test(test_cases = 60)]
fn one_method_is_described_as_the_contract_has_it(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let all = contract()["methods"].as_array().unwrap();
    let m = &all[tc.draw(gs::integers::<usize>().max_value(all.len() - 1))];
    let names: Vec<&str> =
        std::iter::once(&m["name"]).chain(m["aliases"].as_array().unwrap()).filter_map(Value::as_str).collect();
    let name = names[tc.draw(gs::integers::<usize>().max_value(names.len() - 1))];
    let one = describe(Some(name)).unwrap();
    assert_eq!(one["methods"], json!([m]), "{name}");
    assert_eq!(one["version"], env!("CARGO_PKG_VERSION"));
    let defs = one["$defs"].as_object().unwrap();
    let mut used = Vec::new();
    refs(&one, &mut used);
    let used: std::collections::BTreeSet<String> = used.into_iter().collect();
    let defined: std::collections::BTreeSet<String> = defs.keys().cloned().collect();
    assert_eq!(used, defined, "{name}: used and kept differ");
    for (k, d) in defs {
        assert_eq!(d, &contract()["$defs"][k], "{name}: {k}");
    }
    let typo = format!("{name}{}", tc.draw(gs::sampled_from(&[".x", "s", " ", "!"][..])));
    if registry().get(&typo).is_none() {
        assert!(describe(Some(&typo)).is_none(), "{typo}");
    }
}
