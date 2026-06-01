use std::sync::Arc;

use bytes::BytesMut;
use fortrust_core::{PrivacyConfig, PrivacyEngine, RequestContext, ResourceType};
use fortrust_ipc::{BincodeCodec, NetProcessCommand, NetProcessEvent};
use fortrust_net::NetworkClient;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::Mutex;
use tracing::{debug, error, info, warn};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    info!("NetProc: Network process starting");

    let privacy = PrivacyConfig::default();
    let privacy_engine = PrivacyEngine::new(privacy);
    let network = Arc::new(Mutex::new(
        NetworkClient::new(privacy_engine).expect("Failed to create network client"),
    ));

    let listener = match tokio::net::TcpListener::bind("127.0.0.1:0").await {
        Ok(listener) => {
            info!("NetProc listening on {}", listener.local_addr().unwrap());
            listener
        }
        Err(error) => {
            error!("Failed to bind: {error}");
            std::process::exit(1);
        }
    };

    let addr = listener.local_addr().unwrap();
    eprintln!("NETPROC_ADDR={}", addr);

    loop {
        match listener.accept().await {
            Ok((stream, peer)) => {
                debug!("Connection from {peer}");
                let network = Arc::clone(&network);
                tokio::spawn(handle_connection(stream, network));
            }
            Err(error) => {
                warn!("Accept error: {error}");
            }
        }
    }
}

async fn handle_connection(stream: tokio::net::TcpStream, network: Arc<Mutex<NetworkClient>>) {
    let (reader, mut writer) = stream.into_split();
    let mut reader = tokio::io::BufReader::new(reader);
    let mut buf = BytesMut::with_capacity(4096);
    let mut need_data = true;

    loop {
        if need_data {
            buf.reserve(4096);
            if reader.read_buf(&mut buf).await.is_err() || buf.is_empty() {
                break;
            }
        }

        let command = match BincodeCodec::read_raw_payload(&mut buf) {
            Ok(Some(payload)) => {
                need_data = false;
                match BincodeCodec::decode::<NetProcessCommand>(&payload) {
                    Ok(cmd) => cmd,
                    Err(_) => {
                        warn!("Failed to decode command from payload, skipping");
                        continue;
                    }
                }
            }
            Ok(None) => {
                need_data = true;
                continue;
            }
            Err(_) => {
                warn!("Payload read error");
                break;
            }
        };

        match command {
            NetProcessCommand::FetchUrl {
                request_id,
                url,
                headers: _,
                method: _,
                resource_type,
                top_level_url,
            } => {
                let resource_type = match resource_type.as_str() {
                    "document" => ResourceType::Document,
                    "script" => ResourceType::Script,
                    "image" => ResourceType::Image,
                    "stylesheet" => ResourceType::Stylesheet,
                    "xhr" => ResourceType::Xhr,
                    "media" => ResourceType::Media,
                    "font" => ResourceType::Font,
                    _ => ResourceType::Other,
                };

                let context = RequestContext {
                    url,
                    top_level_url,
                    resource_type,
                    referrer_policy: None,
                };

                let mut net = network.lock().await;
                let result = net.fetch(context).await;

                match result {
                    Ok(response) => {
                        let event = NetProcessEvent::ResponseBody {
                            request_id,
                            chunk: response.body.to_vec(),
                            last: true,
                        };
                        let data = BincodeCodec::encode(&event).unwrap_or_default();
                        let _ = writer.write_all(&data).await;

                        let complete = NetProcessEvent::RequestComplete {
                            request_id,
                            status: response.status,
                            total_bytes: response.body.len() as u64,
                            source: format!("{:?}", response.source),
                        };
                        let data = BincodeCodec::encode(&complete).unwrap_or_default();
                        let _ = writer.write_all(&data).await;
                    }
                    Err(error) => {
                        let event = NetProcessEvent::RequestFailed {
                            request_id,
                            error: format!("{error:?}"),
                        };
                        let data = BincodeCodec::encode(&event).unwrap_or_default();
                        let _ = writer.write_all(&data).await;
                    }
                }
            }
            NetProcessCommand::ClearCache => {
                info!("Cache clear requested");
            }
            NetProcessCommand::Shutdown => {
                info!("NetProc shutdown requested");
                let _ = writer
                    .write_all(
                        &BincodeCodec::encode(&NetProcessEvent::ShutdownAck).unwrap_or_default(),
                    )
                    .await;
                break;
            }
            _ => {
                debug!("Unhandled command: {command:?}");
            }
        }
    }
}
