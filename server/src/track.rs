use std::sync::Arc;

use tokio::sync::mpsc;
use webrtc::{
    rtp_transceiver::rtp_codec::RTPCodecType,
    track::{
        track_local::{TrackLocalWriter, track_local_static_rtp::TrackLocalStaticRTP},
        track_remote::TrackRemote,
    },
};

use crate::{error, peer::PeerTrack};

pub struct Track {
    pub inner: PeerTrack,
    pub kind: RTPCodecType,
}

#[derive(Clone)]
pub struct Sender {
    peer_id: u32,
    error_tx: error::Sender,
    tx: mpsc::Sender<Track>,
}

impl Sender {
    pub fn send(self, remote: Arc<TrackRemote>) {
        self.error_tx.spawn(async move {
            let kind = remote.kind();
            let local = Arc::new(TrackLocalStaticRTP::new(
                remote.codec().capability,
                format!("{}-{}", self.peer_id.to_string(), kind),
                remote.stream_id(),
            ));
            self.tx
                .send(Track {
                    inner: PeerTrack {
                        inner: Arc::clone(&local),
                        ssrc: remote.ssrc(),
                    },
                    kind,
                })
                .await?;
            while let Ok((rtp, _)) = remote.read_rtp().await {
                if let Err(e) = local.write_rtp(&rtp).await {
                    if e != webrtc::Error::ErrClosedPipe {
                        return Err(e.into());
                    }
                }
            }
            Ok(())
        });
    }
}

pub struct Receiver {
    rx: mpsc::Receiver<Track>,
}

impl Receiver {
    pub async fn recv(&mut self) -> Option<Track> {
        self.rx.recv().await
    }
}

pub fn channel(peer_id: u32, error_tx: error::Sender) -> (Sender, Receiver) {
    let (tx, rx) = mpsc::channel(2);
    (
        Sender {
            peer_id,
            tx,
            error_tx,
        },
        Receiver { rx },
    )
}
