use tokio::sync::mpsc;

use crate::{signal::ServerMessage, track::Track};

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("serde error: {0}")]
    Serde(#[from] serde_json::Error),

    #[error("axum error: {0}")]
    Axum(#[from] axum::Error),

    #[error("webrtc error: {0}")]
    Webrtc(#[from] webrtc::error::Error),

    #[error("signal send error: {0}")]
    SignalSend(#[from] mpsc::error::SendError<ServerMessage>),

    #[error("track send error: {0}")]
    TrackSend(#[from] mpsc::error::SendError<Track>),
}

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Clone)]
pub struct Sender {
    tx: mpsc::Sender<Error>,
}

impl Sender {
    pub async fn send(&self, fut: impl Future<Output = Result<()>>) {
        if let Err(e) = fut.await {
            self.tx.send(e).await.unwrap();
        }
    }

    pub fn spawn(self, fut: impl Future<Output = Result<()>> + Send + 'static) {
        tokio::spawn(async move {
            self.send(fut).await;
        });
    }
}

pub struct Receiver {
    rx: mpsc::Receiver<Error>,
}

impl Receiver {
    pub async fn recv(&mut self) -> Option<Error> {
        self.rx.recv().await
    }
}

pub fn channel() -> (Sender, Receiver) {
    let (tx, rx) = mpsc::channel(4);
    return (Sender { tx }, Receiver { rx });
}
