use axum::{
    Router,
    extract::ws::{WebSocket, WebSocketUpgrade},
    response::Response,
    routing::get,
};
use peer::Peer;
use signal::Message;
use tokio::net::TcpListener;
use webrtc::peer_connection::peer_connection_state::RTCPeerConnectionState;

mod peer;
mod signal;

#[tokio::main]
async fn main() {
    let app = Router::new().route("/ws", get(handler));
    let listener = TcpListener::bind("0.0.0.0:3000").await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

async fn handler(ws: WebSocketUpgrade) -> Response {
    ws.on_upgrade(handle_socket)
}

async fn handle_socket(socket: WebSocket) {
    let (mut sender, mut receiver) = signal::channel(socket);
    let s1 = sender.clone();
    let peer = Peer::new().await;
    peer.on_connection_state_change(Box::new(|state| {
        Box::pin(async move {
            if state == RTCPeerConnectionState::Connected {
                println!("connection established");
            }
        })
    }));
    peer.on_ice_candidate(Box::new(move |candidate| {
        let mut s2 = s1.clone();
        Box::pin(async move {
            if let Some(candidate) = candidate {
                s2.send(Message::Candidate(candidate.to_json().unwrap()))
                    .await;
            }
        })
    }));
    while let Some(message) = receiver.recv().await {
        let message = if let Some(message) = message {
            message
        } else {
            continue;
        };
        match message {
            Message::Candidate(candidate) => {
                peer.add_ice_candidate(candidate).await;
            }
            Message::Sdp(offer) => {
                peer.set_offer(offer).await;
                let answer = peer.create_answer().await;
                sender.send(Message::Sdp(answer)).await;
            }
        }
    }
}
