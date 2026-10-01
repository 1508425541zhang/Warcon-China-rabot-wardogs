use serde_json::Value;
use warcon_backend::committee::{self, Committee, Input, Quality, Verdict};
#[test]
fn original_five_expert_decisions_and_voting_are_preserved() {
    let fixture: Value = serde_json::from_str(include_str!("../fixtures/committee.json")).unwrap();
    for (index, case) in fixture["assessments"]
        .as_array()
        .unwrap()
        .iter()
        .enumerate()
    {
        let input: Input = serde_json::from_value(case["input"].clone()).unwrap();
        let quality: Quality = serde_json::from_value(case["quality"].clone()).unwrap();
        assert_eq!(
            serde_json::to_value(committee::assess(&input, &quality)).unwrap(),
            serde_json::to_value(
                serde_json::from_value::<Committee>(case["expected"].clone()).unwrap()
            )
            .unwrap(),
            "assessment {index}"
        );
    }
    for (index, case) in fixture["votes"].as_array().unwrap().iter().enumerate() {
        let votes: Vec<Verdict> = serde_json::from_value(case["verdicts"].clone()).unwrap();
        let veto: Vec<String> = serde_json::from_value(case["veto"].clone()).unwrap();
        assert_eq!(
            committee::direct_kick(&votes, 4.01),
            case["direct"].as_bool().unwrap(),
            "kick support {index}"
        );
        assert_eq!(
            serde_json::to_value(committee::vote(votes, veto)).unwrap(),
            serde_json::to_value(
                serde_json::from_value::<Committee>(case["expected"].clone()).unwrap()
            )
            .unwrap(),
            "vote {index}"
        );
    }
    for (index, case) in fixture["saved"].as_array().unwrap().iter().enumerate() {
        assert_eq!(
            committee::statistical_anomaly(&case["assessment"]),
            case["anomaly"].as_bool().unwrap(),
            "persistence anomaly {index}"
        );
        assert_eq!(
            committee::should_save(&case["assessment"]),
            case["save"].as_bool().unwrap(),
            "save decision {index}"
        );
    }
}
