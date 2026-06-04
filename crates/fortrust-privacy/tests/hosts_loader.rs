use fortrust_privacy::blocker::FilterListProvider;

#[test]
fn loads_hosts_and_blocks_known_domain() {
    let mut provider = FilterListProvider::new();
    let sample = b"# sample hosts\n0.0.0.0 example-tracker.test\n127.0.0.1 ads.example.test\n";
    provider.load_hosts_from_bytes(sample);
    assert!(matches!(provider.check_url("https://example-tracker.test/s.js", "https://example.test/", "script"), fortrust_privacy::blocker::BlockerDecision::Block{..}));
    assert!(matches!(provider.check_url("https://ads.example.test/iframe", "https://example.test/", "iframe"), fortrust_privacy::blocker::BlockerDecision::Block{..}));
}
