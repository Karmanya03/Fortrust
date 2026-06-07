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
        "https://analytics.twitter.com/i/adsct",
        "https://www.google.com/pagead/1p-user-list/",
        "https://cdn.taboola.com/libtrc/",
        "https://example.com/ads/banner.gif",
    ];
    for u in urls { println!("  {} -> {}", u, filter.should_block(u, "https://example.com", "image")); }
    // Try with script type
    for u in urls { println!("  {} (script) -> {}", u, filter.should_block(u, "https://example.com", "script")); }
}
