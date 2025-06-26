use anyhow::{anyhow, Result as AnyResult};
use base64::{engine::general_purpose, Engine as _};
use futures::{FutureExt, StreamExt};
use uuid::Uuid;
use wasm_bindgen::JsValue;
use worker::*;

mod vless;

const UUID: &str = "0700dfc1-6242-590e-b49a-03328379e43f";
const READ_BUFFER_SIZE: usize = 4096;

#[event(fetch)]
async fn fetch(req: HttpRequest, env: Env, _ctx: Context) -> Result<worker::Response> {
    let uuid_text = env.var("UUID").map(|v| v.to_string()).unwrap_or_else(|_| UUID.to_string());
    let user_id = uuid::Uuid::parse_str(&uuid_text).map_err(|_| worker::Error::Internal(JsValue::NULL))?;
    let default_vless_path = format!("/vless/{}", &uuid_text);
    let vless_path = env.var("VLESS_PATH").map(|v| v.to_string()).unwrap_or_else(|_| default_vless_path);

    let path = req.uri().path();

    if path == vless_path {
        return handle_vless_connection(req, user_id).await;
    }

    // TODO: subscription? and fake index?

    worker::Response::error(format!("Unknown path: {}", path), 404)
}

async fn handle_vless_connection(req: HttpRequest, user_id: Uuid) -> Result<worker::Response> {
    let upgrade_header = req.headers().get("Upgrade").map(|v| v.to_str().unwrap()).unwrap_or_default();
    if upgrade_header != "websocket" {
        return worker::Response::error("Expected Upgrade: websocket", 426);
    }
    let early_data = req.headers().get("sec-websocket-protocol").map(|v| v.to_str().unwrap().to_string());

    let WebSocketPair { client, server } = WebSocketPair::new()?;
    server.accept()?;

    wasm_bindgen_futures::spawn_local(async move {
        if let Err(e) = handle_ws_connection(&server, user_id, early_data).await {
            console_error!("Connection handler failed: {}", e);
        };
        let _ = server.close::<&str>(None, None);
    });
    worker::Response::from_websocket(client)
}

async fn handle_ws_connection(server: &WebSocket, user_id: Uuid, early_data: Option<String>) -> AnyResult<()> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let mut event_stream = server.events()?;
    // get vless header data from early_data or first msg
    let mut vless_header_bytes = match early_data {
        Some(d) => general_purpose::STANDARD.decode(d)?,
        None => match event_stream.next().await {
            Some(Ok(WebsocketEvent::Message(msg))) => msg.bytes().ok_or_else(|| anyhow!("Failed to read 1st chunk"))?,
            _ => return Err(anyhow!("No 1st chunk received")),
        },
    };

    // parse vless header and possible request data left in first msg
    let vless_header = vless::process_vless_header(&vless_header_bytes, user_id)?;
    let request_data = vless_header_bytes.split_off(vless_header.len);

    // handle udp command only for dns requests
    if vless_header.command == vless::VlessCommand::Udp {
        if vless_header.port != 53 {
            return Err(anyhow!("Unsupported UDP request for port {}", vless_header.port));
        }
        // handle DNS over UDP
        return Err(anyhow!("DNS request not implemented yet"));
        // return handle_dns_over_udp(server, event_stream).await;
    }

    // connect to remote server, and send request_data if available
    let hostname = vless_header.address.to_string();
    let port = vless_header.port;
    let mut remote_socket = ConnectionBuilder::new().connect(hostname, port)?;
    if !request_data.is_empty() {
        remote_socket.write_all(&request_data).await?;
    }

    // send vless response header to client
    server.send_with_bytes(vless::get_vless_response_header())?;

    // proxy data between client and remote server
    let mut read_buf = [0u8; READ_BUFFER_SIZE];
    loop {
        futures::select! {
            event = event_stream.next().fuse() => {
                match event {
                    Some(Ok(WebsocketEvent::Message(msg))) => {
                        match msg.bytes() {
                            Some(bytes) => remote_socket.write_all(&bytes).await?, // forward client data to remote svr
                            None => return Err(anyhow!("Received non-bytes message from client")),
                        }
                    }
                    Some(Ok(WebsocketEvent::Close(_))) => break, // Client closed the connection
                    Some(Err(e)) => return Err(anyhow!("WebSocket error: {}", e)),
                    None => break, // WebSocket closed
                }
            }
            data = remote_socket.read(&mut read_buf).fuse() => {
                match data {
                    Ok(0) => break, // Remote socket closed
                    Ok(n) => server.send_with_bytes(&read_buf[..n])?, // Forward data to client
                    Err(e) => return Err(anyhow!("Error reading from remote socket: {}", e)),
                }

            }
        }
    }
    Ok(())
}

// TODO: need to confirm dns message and vless udp message format
// async fn handle_dns_over_udp(server: &WebSocket, mut event_stream: EventStream<'_>) -> AnyResult<()> {
//     loop {
//         futures::select! {
//             event = event_stream.next().fuse() => {
//                 match event {
//                     Some(Ok(WebsocketEvent::Message(msg))) => {
//                         match msg.bytes() {
//                             Some(bytes) => remote_socket.write_all(&bytes).await?, // forward client data to remote svr
//                             None => return Err(anyhow!("Received non-bytes message from client")),
//                         }
//                     }
//                     Some(Ok(WebsocketEvent::Close(_))) => break, // Client closed the connection
//                     Some(Err(e)) => return Err(anyhow!("WebSocket error: {}", e)),
//                     None => break, // WebSocket closed
//                 }
//             }
//             data = remote_socket.read(&mut read_buf).fuse() => {
//                 match data {
//                     Ok(0) => break, // Remote socket closed
//                     Ok(n) => server.send_with_bytes(&read_buf[..n])?, // Forward data to client
//                     Err(e) => return Err(anyhow!("Error reading from remote socket: {}", e)),
//                 }

//             }
//         }
//     }
//     Err(anyhow!("DNS over UDP is not implemented yet"))
// }
