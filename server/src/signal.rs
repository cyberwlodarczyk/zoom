use axum::extract::ws::{Message as WebSocketMessage, WebSocket};
use futures::{
    sink::SinkExt,
    stream::{SplitStream, StreamExt},
};
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use webrtc::ice_transport::ice_candidate::RTCIceCandidateInit;

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ServerMessagePeer {
    pub id: u32,
    pub name: String,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub enum ServerMessage {
    Candidate(RTCIceCandidateInit),
    Offer(String),
    Answer(String),
    Id(u32),
    Peers(Vec<ServerMessagePeer>),
    Peer(ServerMessagePeer),
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub enum PeerMessage {
    Candidate(RTCIceCandidateInit),
    Offer(String),
    Answer(String),
    Name(String),
    Pli(bool),
}

#[derive(Clone)]
pub struct Sender {
    tx: mpsc::Sender<ServerMessage>,
}

impl Sender {
    pub async fn send(&mut self, message: ServerMessage) {
        self.tx.send(message).await.unwrap();
    }
}

pub struct Receiver {
    stream: SplitStream<WebSocket>,
}

impl Receiver {
    pub async fn recv(&mut self) -> Option<PeerMessage> {
        if let WebSocketMessage::Text(text) = self.stream.next().await?.unwrap() {
            Some(serde_json::from_str::<PeerMessage>(&text).unwrap())
        } else {
            None
        }
    }
}

pub fn channel(socket: WebSocket) -> (Sender, Receiver) {
    let (mut sink, stream) = socket.split();
    let (tx, mut rx) = mpsc::channel(4);
    tokio::spawn(async move {
        while let Some(message) = rx.recv().await {
            sink.send(WebSocketMessage::Text(
                serde_json::to_string(&message).unwrap().into(),
            ))
            .await
            .unwrap();
        }
    });
    (Sender { tx }, Receiver { stream })
}
