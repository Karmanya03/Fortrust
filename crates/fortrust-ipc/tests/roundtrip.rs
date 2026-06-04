use fortrust_ipc::{
    BincodeCodec, BrowserToRenderer, KeyEvent, LoadState, MouseEvent, NetProcessCommand,
    NetProcessEvent, PrivacyEvent, RendererToBrowser, create_ipc_pair,
};

#[tokio::test]
async fn browser_to_renderer_navigate_roundtrip() {
    // create_ipc_pair cross-wires: channel_a.tx → channel_b.rx, channel_b.tx → channel_a.rx.
    // Keep both sides alive to make send→receive work.
    let (a, b) = create_ipc_pair();
    let (tx_a, _rx_a) = a.split();
    let (_tx_b, rx_b) = b.split();

    let msg = BrowserToRenderer::Navigate { url: "https://example.com".into() };
    tx_a.send_browser_message(&msg).await.unwrap();
    let received: BrowserToRenderer = rx_b.recv_browser_message().await.unwrap();
    assert!(matches!(received, BrowserToRenderer::Navigate { url } if url == "https://example.com"));
}

#[tokio::test]
async fn browser_to_renderer_full_enum_roundtrip() {
    let (a, b) = create_ipc_pair();
    let (tx_a, _rx_a) = a.split();
    let (_tx_b, rx_b) = b.split();

    let cases = vec![
        BrowserToRenderer::Navigate { url: "https://test.org".into() },
        BrowserToRenderer::GoBack,
        BrowserToRenderer::GoForward,
        BrowserToRenderer::Reload,
        BrowserToRenderer::Stop,
        BrowserToRenderer::ExecuteScript { js: "alert(1)".into() },
        BrowserToRenderer::KeyEvent {
            event: KeyEvent { key: "a".into(), code: "KeyA".into(), ctrl: true, alt: false, shift: false, meta: false },
        },
        BrowserToRenderer::MouseEvent {
            event: MouseEvent { x: 100.0, y: 200.0, button: 0, buttons: 1, ctrl: false, alt: false, shift: false, meta: false },
        },
        BrowserToRenderer::Resize { width: 1920, height: 1080 },
        BrowserToRenderer::ZoomChange { factor: 1.5 },
        BrowserToRenderer::ScrollTo { x: 50.0, y: 100.0 },
        BrowserToRenderer::SetPrivacySettings { block_ads: true, block_trackers: true, https_only: true },
        BrowserToRenderer::Shutdown,
    ];
    for msg in cases {
        tx_a.send_browser_message(&msg).await.unwrap();
        let received = rx_b.recv_browser_message().await.unwrap();
        assert_eq!(format!("{msg:?}"), format!("{received:?}"), "Mismatch for {msg:?}");
    }
}

#[tokio::test]
async fn renderer_to_browser_full_enum_roundtrip() {
    let (a, b) = create_ipc_pair();
    let (_tx_a, rx_a) = a.split();
    let (tx_b, _rx_b) = b.split();

    let cases = vec![
        RendererToBrowser::TitleChanged { title: "Test Page".into() },
        RendererToBrowser::UrlChanged { url: "https://example.com".into() },
        RendererToBrowser::FaviconUpdated { data: vec![1, 2, 3] },
        RendererToBrowser::LoadProgress { percent: 0.5, state: LoadState::Loading },
        RendererToBrowser::LoadComplete,
        RendererToBrowser::FrameReady { texture_data: vec![0u8; 64], width: 8, height: 8, stride: 8 },
        RendererToBrowser::PrivacyEvent {
            event: PrivacyEvent::AdBlocked { url: "https://ads.com/pixel".into() },
        },
        RendererToBrowser::Alert { message: "Hello".into() },
        RendererToBrowser::ConsoleMessage { level: "warn".into(), message: "test".into() },
        RendererToBrowser::NewTabRequested { url: "https://newtab.com".into() },
        RendererToBrowser::DownloadRequested {
            url: "https://dl.com/file".into(), filename: "file.zip".into(), mime_type: "application/zip".into()
        },
        RendererToBrowser::NavigationStart { url: "https://nav.com".into() },
        RendererToBrowser::NavigationError { url: "https://bad.com".into(), error: "timeout".into() },
        RendererToBrowser::DocumentTitleChanged { title: "New Title".into() },
        RendererToBrowser::ScrollPosition { x: 10.0, y: 20.0 },
        RendererToBrowser::RendererCrashed { reason: "OOM".into() },
        RendererToBrowser::MemoryUsage { used_mb: 128, heap_mb: 256 },
        RendererToBrowser::ShutdownAck,
    ];
    for msg in cases {
        tx_b.send_renderer_message(&msg).await.unwrap();
        let received = rx_a.recv_renderer_message().await.unwrap();
        assert_eq!(format!("{msg:?}"), format!("{received:?}"), "Mismatch for {msg:?}");
    }
}

#[tokio::test]
async fn net_command_full_enum_roundtrip() {
    let (a, b) = create_ipc_pair();
    let (tx_a, _rx_a) = a.split();
    let (_tx_b, rx_b) = b.split();

    let cases = vec![
        NetProcessCommand::FetchUrl {
            request_id: 1, url: "https://example.com".into(),
            headers: vec![("Accept".into(), "text/html".into())],
            method: "GET".into(), resource_type: "document".into(),
            top_level_url: Some("https://example.com".into()),
        },
        NetProcessCommand::FetchStream { request_id: 2, url: "https://cdn.com/video.mp4".into(), headers: vec![] },
        NetProcessCommand::CancelRequest { request_id: 3 },
        NetProcessCommand::SetDohProvider { provider: "cloudflare".into() },
        NetProcessCommand::ClearCache,
        NetProcessCommand::PrefetchUrls { urls: vec!["https://prefetch.com".into()] },
        NetProcessCommand::Shutdown,
    ];
    for msg in cases {
        tx_a.send_net_command(&msg).await.unwrap();
        let received = rx_b.recv_net_command().await.unwrap();
        assert_eq!(format!("{msg:?}"), format!("{received:?}"), "Mismatch for {msg:?}");
    }
}

#[tokio::test]
async fn net_event_full_enum_roundtrip() {
    let (a, b) = create_ipc_pair();
    let (tx_a, _rx_a) = a.split();
    let (_tx_b, rx_b) = b.split();

    let cases = vec![
        NetProcessEvent::ResponseHeaders { request_id: 1, status: 200, headers: vec![("Content-Type".into(), "text/html".into())] },
        NetProcessEvent::ResponseBody { request_id: 1, chunk: vec![72, 101, 108], last: false },
        NetProcessEvent::RequestComplete { request_id: 1, status: 200, total_bytes: 1024, source: "network".into() },
        NetProcessEvent::RequestFailed { request_id: 2, error: "connection refused".into() },
        NetProcessEvent::RequestBlocked { request_id: 3, reason: "malware".into(), original_url: "https://bad.com".into() },
        NetProcessEvent::CacheHit { request_id: 4, cached_bytes: 512 },
        NetProcessEvent::ShutdownAck,
    ];
    for msg in cases {
        tx_a.send_net_event(&msg).await.unwrap();
        let received = rx_b.recv_net_event().await.unwrap();
        assert_eq!(format!("{msg:?}"), format!("{received:?}"), "Mismatch for {msg:?}");
    }
}

#[tokio::test]
async fn bidirectional_communication() {
    let (a, b) = create_ipc_pair();
    let (tx_a, rx_a) = a.split();
    let (tx_b, rx_b) = b.split();

    let browser_msg = BrowserToRenderer::Navigate { url: "https://example.com".into() };
    tx_a.send_browser_message(&browser_msg).await.unwrap();
    let renderer_msg = RendererToBrowser::TitleChanged { title: "Hello".into() };
    tx_b.send_renderer_message(&renderer_msg).await.unwrap();

    let received_by_b = rx_b.recv_browser_message().await.unwrap();
    let received_by_a = rx_a.recv_renderer_message().await.unwrap();
    assert!(matches!(received_by_b, BrowserToRenderer::Navigate { url } if url == "https://example.com"));
    assert!(matches!(received_by_a, RendererToBrowser::TitleChanged { title } if title == "Hello"));
}

#[tokio::test]
async fn codec_serialization_roundtrip() {
    let original = BrowserToRenderer::Navigate { url: "https://serde.test/path?q=1".into() };
    let encoded = BincodeCodec::encode(&original).unwrap();
    let decoded: BrowserToRenderer = BincodeCodec::decode(&encoded[8..]).unwrap();
    assert_eq!(format!("{original:?}"), format!("{decoded:?}"));

    let original_net = NetProcessCommand::FetchUrl {
        request_id: 42, url: "https://net.test/data".into(),
        headers: vec![("Authorization".into(), "Bearer token".into())],
        method: "POST".into(), resource_type: "xhr".into(),
        top_level_url: None,
    };
    let encoded = BincodeCodec::encode(&original_net).unwrap();
    let decoded: NetProcessCommand = BincodeCodec::decode(&encoded[8..]).unwrap();
    assert_eq!(format!("{original_net:?}"), format!("{decoded:?}"));
}

#[tokio::test]
async fn empty_payloads_roundtrip() {
    let (a, b) = create_ipc_pair();
    let (tx_a, _rx_a) = a.split();
    let (_tx_b, rx_b) = b.split();

    tx_a.send_browser_message(&BrowserToRenderer::Stop).await.unwrap();
    let recv = rx_b.recv_browser_message().await.unwrap();
    assert!(matches!(recv, BrowserToRenderer::Stop));

    tx_a.send_renderer_message(&RendererToBrowser::ShutdownAck).await.unwrap();
    let recv2 = rx_b.recv_renderer_message().await.unwrap();
    assert!(matches!(recv2, RendererToBrowser::ShutdownAck));
}
