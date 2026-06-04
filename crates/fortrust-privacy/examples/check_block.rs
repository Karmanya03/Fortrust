use fortrust_privacy::PrivacyFilter;
fn main() {
    let filter = PrivacyFilter::load();
    let urls = [
        "https://pagead2.googlesyndication.com/pagead/js/adsbygoogle.js",
        "https://securepubads.g.doubleclick.net/gampad/ads",
        "https://ad.doubleclick.net/ddm/trackimp",
        "https://www.google-analytics.com/analytics.js",
        "https://www.googletagmanager.com/gtm.js",
        "https://connect.facebook.net/en_US/fbevents.js",
        "https://doubleclick.net/pixel?id=123",
        "https://example.com/ads/banner.gif",
        "https://example.com/analytics/collect",
    ];
    println!("=== resource_type: image ===");
    for u in &urls { println!("  {u} -> {}", filter.should_block(u, "https://example.com", "image")); }
    println!("=== resource_type: script ===");
    for u in &urls { println!("  {u} -> {}", filter.should_block(u, "https://example.com", "script")); }
    println!("=== resource_type: sub_frame ===");
    for u in &urls { println!("  {u} -> {}", filter.should_block(u, "https://example.com", "sub_frame")); }
}
