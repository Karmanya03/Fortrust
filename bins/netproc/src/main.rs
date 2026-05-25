use std::sync::Arc;

use fortrust_core::{PrivacyConfig, PrivacyEngine, RequestContext, ResourceType};
use fortrust_ipc::{BincodeCodec, NetProcessCommand, NetProcessEvent};
use fortrust_net::NetworkClient;
use tokio::io::{AsyncReadExt, BufReader};
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
    use tokio::io::AsyncWriteExt;

    let (reader, mut writer) = stream.into_split();
    let mut reader = BufReader::new(reader);
    let mut buf = Vec::new();

    loop {
        buf.clear();
        match reader.read(&mut buf).await {
            Ok(0) => break,
            Ok(_) => {}
            Err(error) => {
                warn!("Read error: {error}");
                break;
            }
        }

        if buf.is_empty() {
            continue;
        }

        let Ok(command) = BincodeCodec::decode::<NetProcessCommand>(&buf) else {
            warn!("Failed to decode command");
            continue;
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
