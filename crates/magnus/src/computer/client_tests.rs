use super::*;
use std::sync::{Arc, Mutex};
use tokio::net::UnixListener;

/// What a fake computerd does with each connection, in order.
#[derive(Clone, Copy)]
enum Turn {
    /// The relay could not reach the guest: nothing is read past the connect.
    Unreached,
    /// The guest takes the request and answers it.
    Answers,
    /// The guest takes the request and goes away without answering.
    TakesAndCloses,
}

struct Fake {
    _dir: tempfile::TempDir,
    socket: std::path::PathBuf,
    connections: Arc<Mutex<usize>>,
    requests: Arc<Mutex<Vec<Value>>>,
}

fn fake(turns: &[Turn]) -> Fake {
    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("computerd.sock");
    let listener = UnixListener::bind(&socket).unwrap();
    let (connections, requests) = (Arc::new(Mutex::new(0)), Arc::new(Mutex::new(Vec::new())));
    let (c, r, turns) = (connections.clone(), requests.clone(), turns.to_vec());
    tokio::spawn(async move {
        for turn in turns {
            let (s, _) = listener.accept().await.unwrap();
            *c.lock().unwrap() += 1;
            let (read, mut write) = s.into_split();
            let mut read = BufReader::new(read);
            let mut line = String::new();
            read.read_line(&mut line).await.unwrap();
            assert_eq!(serde_json::from_str::<Value>(&line).unwrap()["op"], "connect");
            if let Turn::Unreached = turn {
                write.write_all(b"{\"ok\":false,\"error\":\"Connection reset by peer (os error 104)\"}\n").await.unwrap();
                continue;
            }
            write.write_all(b"{\"ok\":true}\n").await.unwrap();
            line.clear();
            read.read_line(&mut line).await.unwrap();
            r.lock().unwrap().push(serde_json::from_str(&line).unwrap());
            if let Turn::Answers = turn {
                write.write_all(b"{\"ok\":true,\"answered\":true}\n").await.unwrap();
            }
        }
    });
    Fake { _dir: dir, socket, connections, requests }
}

#[tokio::test]
async fn a_connection_that_never_reached_the_guest_is_opened_again() {
    let f = fake(&[Turn::Unreached, Turn::Unreached, Turn::Answers]);
    let reply = guest(&f.socket, "c1", json!({ "op": "write", "path": "/workspace/a" })).await.unwrap();
    assert_eq!(reply["answered"], true);
    assert_eq!(*f.connections.lock().unwrap(), 3);
    assert_eq!(f.requests.lock().unwrap().len(), 1, "the request went more than once");
}

#[tokio::test]
async fn a_guest_never_reached_is_tried_a_bounded_number_of_times() {
    let f = fake(&[Turn::Unreached; GUEST_CONNECT_TRIES as usize + 1]);
    let e = guest(&f.socket, "c1", json!({ "op": "ping" })).await.unwrap_err();
    assert!(e.to_string().contains("reset by peer"), "{e:#}");
    assert_eq!(*f.connections.lock().unwrap(), GUEST_CONNECT_TRIES as usize);
    assert!(f.requests.lock().unwrap().is_empty());
}

#[tokio::test]
async fn a_request_once_sent_is_never_sent_again() {
    let f = fake(&[Turn::TakesAndCloses, Turn::Answers]);
    let e = guest(&f.socket, "c1", json!({ "op": "write", "path": "/workspace/a", "append": true })).await.unwrap_err();
    assert!(e.to_string().contains("closed the connection"), "{e:#}");
    assert_eq!(*f.connections.lock().unwrap(), 1, "a request the guest may have run was sent again");
    assert_eq!(f.requests.lock().unwrap().len(), 1);
}
