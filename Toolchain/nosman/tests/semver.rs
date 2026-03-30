use nosman::nosman::index::SemVer;

#[test]
fn test_matches_prefix() {
    // Test major only prefix (6 matches 6.x.x)
    let prefix = SemVer::parse_from_str("6").unwrap();
    assert!(SemVer::parse_from_str("6.0.0").unwrap().matches_prefix(&prefix));
    assert!(SemVer::parse_from_str("6.30.1").unwrap().matches_prefix(&prefix));
    assert!(SemVer::parse_from_str("6.99.99").unwrap().matches_prefix(&prefix));
    assert!(!SemVer::parse_from_str("5.99.99").unwrap().matches_prefix(&prefix));
    assert!(!SemVer::parse_from_str("7.0.0").unwrap().matches_prefix(&prefix));

    // Test major.minor prefix (6.30 matches 6.30.x)
    let prefix = SemVer::parse_from_str("6.30").unwrap();
    assert!(SemVer::parse_from_str("6.30.0").unwrap().matches_prefix(&prefix));
    assert!(SemVer::parse_from_str("6.30.1").unwrap().matches_prefix(&prefix));
    assert!(SemVer::parse_from_str("6.30.99").unwrap().matches_prefix(&prefix));
    assert!(!SemVer::parse_from_str("6.29.99").unwrap().matches_prefix(&prefix));
    assert!(!SemVer::parse_from_str("6.31.0").unwrap().matches_prefix(&prefix));

    // Test major.minor.patch prefix (6.30.1 matches 6.30.1.x)
    let prefix = SemVer::parse_from_str("6.30.1").unwrap();
    assert!(SemVer::parse_from_str("6.30.1").unwrap().matches_prefix(&prefix));
    assert!(SemVer::parse_from_str("6.30.1.b709").unwrap().matches_prefix(&prefix));
    assert!(SemVer::parse_from_str("6.30.1.b999").unwrap().matches_prefix(&prefix));
    assert!(!SemVer::parse_from_str("6.30.0").unwrap().matches_prefix(&prefix));
    assert!(!SemVer::parse_from_str("6.30.2").unwrap().matches_prefix(&prefix));

    // Test full version prefix (6.30.1.b709 matches exactly 6.30.1.b709)
    let prefix = SemVer::parse_from_str("6.30.1.b709").unwrap();
    assert!(SemVer::parse_from_str("6.30.1.b709").unwrap().matches_prefix(&prefix));
    assert!(!SemVer::parse_from_str("6.30.1.b708").unwrap().matches_prefix(&prefix));
    assert!(!SemVer::parse_from_str("6.30.1.b710").unwrap().matches_prefix(&prefix));
}
