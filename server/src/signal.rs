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
pub enum Message {
    Candidate(RTCIceCandidateInit),
    Sdp(String),
}

#[derive(Clone)]
pub struct Sender {
    tx: mpsc::Sender<Message>,
}

impl Sender {
    pub async fn send(&mut self, message: Message) {
        self.tx.send(message).await.unwrap();
    }
}

pub struct Receiver {
    stream: SplitStream<WebSocket>,
}

impl Receiver {
    pub async fn recv(&mut self) -> Option<Option<Message>> {
        Some(
            if let WebSocketMessage::Text(text) = self.stream.next().await?.unwrap() {
                Some(serde_json::from_str::<Message>(&text).unwrap())
            } else {
                None
            },
        )
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
