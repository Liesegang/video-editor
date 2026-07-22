use super::{ActionTracker, QaServer};
use crate::qa::ui_query::UiQueryKind;
use serde_json::json;
use std::io::{Read, Write};
use std::net::{Shutdown, SocketAddr, TcpStream};
use std::sync::mpsc;
use std::sync::Arc;
use std::time::Duration;

fn request(address: SocketAddr, request: &str) -> Result<String, String> {
    let mut stream = TcpStream::connect(address).map_err(|error| error.to_string())?;
    stream
        .write_all(request.as_bytes())
        .map_err(|error| error.to_string())?;
    stream
        .shutdown(Shutdown::Write)
        .map_err(|error| error.to_string())?;
    let mut response = String::new();
    stream
        .read_to_string(&mut response)
        .map_err(|error| error.to_string())?;
    Ok(response)
}

#[test]
fn state_and_probe_endpoints_are_answered_on_demand_by_the_ui_side() -> Result<(), String> {
    let (sender, _receiver) = mpsc::sync_channel(1);
    let (query_sender, query_receiver) = mpsc::sync_channel(1);
    let server = QaServer::start(
        0,
        sender,
        query_sender,
        Arc::new(ActionTracker::default()),
        egui::Context::default(),
    )
    .map_err(|error| error.to_string())?;
    let address = server.address();
    let requester = std::thread::spawn(move || {
        request(address, "GET /v1/state HTTP/1.1\r\nHost: localhost\r\n\r\n")
    });
    let query = query_receiver
        .recv_timeout(Duration::from_secs(1))
        .map_err(|error| error.to_string())?;
    assert!(matches!(&query.kind, UiQueryKind::Snapshot));
    query
        .response
        .send(Ok(
            json!({"frame": 42, "project": {"name": "authoritative"}}),
        ))
        .map_err(|error| error.to_string())?;
    let response = requester
        .join()
        .map_err(|_| "state requester thread panicked".to_string())??;
    assert!(response.starts_with("HTTP/1.1 200 OK"));
    assert!(response.contains("\"frame\":42"));
    assert!(response.contains("\"name\":\"authoritative\""));

    let node_id = uuid::Uuid::new_v4();
    let body = format!("{{\"node_id\":\"{node_id}\",\"port\":\"value\",\"global_time\":1.25}}");
    let requester = std::thread::spawn(move || {
        request(
            address,
            &format!(
                "POST /v1/probes/metadata-output HTTP/1.1\r\nHost: localhost\r\nContent-Length: {}\r\n\r\n{body}",
                body.len()
            ),
        )
    });
    let query = query_receiver
        .recv_timeout(Duration::from_secs(1))
        .map_err(|error| error.to_string())?;
    let probe = match &query.kind {
        UiQueryKind::MetadataOutput(probe) => probe,
        UiQueryKind::Snapshot => {
            return Err("metadata endpoint enqueued a snapshot query".to_string());
        }
    };
    assert_eq!(probe.node_id, node_id);
    query
        .response
        .send(Ok(json!({"result": "runtime"})))
        .map_err(|error| error.to_string())?;
    let response = requester
        .join()
        .map_err(|_| "metadata requester thread panicked".to_string())??;
    assert!(response.starts_with("HTTP/1.1 200 OK"));
    assert!(response.contains("\"result\":\"runtime\""));
    Ok(())
}
