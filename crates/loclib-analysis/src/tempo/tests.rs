use super::*;
use hegel::generators as gs;

#[test]
fn classifies_octave_and_fifth_errors() {
    let r = |a, b| TempoRelation::between(a, b).unwrap();
    assert_eq!(r(128.0, 126.0), TempoRelation::Agrees);
    assert_eq!(r(64.0, 128.0), TempoRelation::Half);
    assert_eq!(r(174.0, 87.0), TempoRelation::Double);
    assert_eq!(r(80.0, 120.0), TempoRelation::TwoThirds);
    assert_eq!(r(180.0, 120.0), TempoRelation::ThreeHalves);
    assert_eq!(r(140.0, 128.0), TempoRelation::Disagrees);
    // 4% either way, not 5%.
    assert_eq!(r(124.0 * 1.039, 124.0), TempoRelation::Agrees);
    assert_eq!(r(124.0 * 1.045, 124.0), TempoRelation::Disagrees);
    assert_eq!(TempoRelation::between(0.0, 128.0), None);
    assert_eq!(TempoRelation::between(128.0, f64::NAN), None);
}

/// Each relation and the stored tempo over the estimate it stands for.
const FACTORS: [(TempoRelation, f64); 5] = [
    (TempoRelation::Agrees, 1.0),
    (TempoRelation::Half, 0.5),
    (TempoRelation::Double, 2.0),
    (TempoRelation::TwoThirds, 2.0 / 3.0),
    (TempoRelation::ThreeHalves, 1.5),
];

/// Swapping the two tempos gives the opposite relation.
#[hegel::test(test_cases = 500)]
fn relations_are_symmetric(tc: hegel::TestCase) {
    let a = tc.draw(gs::floats::<f64>().min_value(20.0).max_value(300.0));
    let b = tc.draw(gs::floats::<f64>().min_value(20.0).max_value(300.0));
    let opposite = |r| match r {
        TempoRelation::Half => TempoRelation::Double,
        TempoRelation::Double => TempoRelation::Half,
        TempoRelation::TwoThirds => TempoRelation::ThreeHalves,
        TempoRelation::ThreeHalves => TempoRelation::TwoThirds,
        same => same,
    };
    assert_eq!(TempoRelation::between(b, a).map(opposite), TempoRelation::between(a, b));
}

/// A tempo off by one of the factors, give or take less than the tolerance, is classed as
/// that factor.
#[hegel::test(test_cases = 500)]
fn factors_are_recognised(tc: hegel::TestCase) {
    let estimate = tc.draw(gs::floats::<f64>().min_value(40.0).max_value(250.0));
    let jitter = tc.draw(gs::floats::<f64>().min_value(-0.035).max_value(0.035));
    let (relation, factor) = tc.draw(gs::sampled_from(&FACTORS));
    let stored = estimate * factor * (1.0 + jitter);
    assert_eq!(TempoRelation::between(stored, estimate), Some(relation));
}

#[test]
fn a_library_prefers_tempos_it_has_many_of() {
    let library = TempoPrior::library([174.0, 172.0, 175.0, 87.0, 0.0, f64::NAN]);
    assert_eq!(library.plausibility(174.0), 0.75);
    assert_eq!(library.more_plausible(87.0, 174.0), Some(Plausible::Estimate));
    assert_eq!(library.more_plausible(140.0, 150.0), None);
    assert_eq!(library.plausibility(-1.0), 0.0);
    assert_eq!(TempoPrior::library([]).plausibility(120.0), 0.0);
}

/// Asking with the two tempos swapped swaps the answer.
#[hegel::test(test_cases = 300)]
fn plausibility_is_antisymmetric(tc: hegel::TestCase) {
    let bpms = tc.draw(gs::vecs(gs::floats::<f64>().min_value(60.0).max_value(200.0)).max_size(20));
    let prior = TempoPrior::library(bpms);
    let a = tc.draw(gs::floats::<f64>().min_value(40.0).max_value(250.0));
    let b = tc.draw(gs::floats::<f64>().min_value(40.0).max_value(250.0));
    let swap =
        |p: Option<Plausible>| p.map(|p| if p == Plausible::Stored { Plausible::Estimate } else { Plausible::Stored });
    assert_eq!(prior.more_plausible(b, a), swap(prior.more_plausible(a, b)));
}

/// A tempo that isn't one (zero, negative, not a number, infinite) relates to nothing and is
/// never plausible, whatever the library.
#[hegel::test(test_cases = 200)]
fn tempos_that_arent_relate_to_nothing(tc: hegel::TestCase) {
    let not =
        tc.draw(hegel::one_of!(gs::floats::<f64>().max_value(0.0), gs::sampled_from(vec![f64::NAN, f64::INFINITY])));
    let bpm = tc.draw(gs::floats::<f64>().min_value(20.0).max_value(300.0));
    assert_eq!((TempoRelation::between(not, bpm), TempoRelation::between(bpm, not)), (None, None));
    let prior =
        TempoPrior::library(tc.draw(gs::vecs(gs::floats::<f64>().min_value(60.0).max_value(200.0)).max_size(10)));
    assert_eq!(prior.plausibility(not), 0.0);
    assert!((0.0..=1.0).contains(&prior.plausibility(bpm)));
}
