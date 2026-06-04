use fortrust_ipc::{BrowserToRenderer, RendererToBrowser};

#[test]
fn tcp_endpoint_roundtrip() {
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().expect("runtime");

    rt.block_on(async {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0)).await.expect("bind");
        let addr = listener.local_addr().expect("local addr");

        let client = tokio::spawn(async move {
            let mut stream = tokio::net::TcpStream::connect(addr).await.expect("connect");
            let mut buffer = bytes::BytesMut::new();
            let mut tmp = [0u8; 4096];

            loop {
                let read = stream.read(&mut tmp).await.expect("client read");
                if read == 0 {
                    break;
                }
                buffer.extend_from_slice(&tmp[..read]);

                if let Ok(Some(payload)) = fortrust_ipc::BincodeCodec::read_raw_payload(&mut buffer) {
                    let msg: BrowserToRenderer = fortrust_ipc::BincodeCodec::decode(&payload).expect("decode nav");
                    match msg {
                        BrowserToRenderer::Navigate { url } => {
                            let reply = RendererToBrowser::TitleChanged { title: url };
                            let data = fortrust_ipc::FramedMessage::new(&reply).expect("frame reply").into_bytes();
                            stream.write_all(&data).await.expect("write reply");
                            break;
                        }
                        other => panic!("unexpected message: {:?}", other),
                    }
                }
            }
        });

        let (stream, _) = listener.accept().await.expect("accept");
        let (sender, receiver) = fortrust_ipc::create_tcp_endpoint(stream);

        sender
            .send_browser_message(&BrowserToRenderer::Navigate { url: "https://example.test/".into() })
            .await
            .expect("send nav");

        let reply = tokio::time::timeout(std::time::Duration::from_secs(5), receiver.recv_renderer_message())
            .await
            .expect("timeout")
            .expect("recv reply");

        match reply {
            RendererToBrowser::TitleChanged { title } => assert_eq!(title, "https://example.test/"),
            other => panic!("unexpected reply: {:?}", other),
        }

        client.await.expect("client task");
    });
}
