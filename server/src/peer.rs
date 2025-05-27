use std::{pin::Pin, sync::Arc};

use once_cell::sync::Lazy;
use webrtc::{
    api::{APIBuilder, media_engine::MediaEngine},
    ice_transport::ice_candidate::RTCIceCandidateInit,
    peer_connection::{
        RTCPeerConnection, configuration::RTCConfiguration,
        peer_connection_state::RTCPeerConnectionState,
        sdp::session_description::RTCSessionDescription, signaling_state::RTCSignalingState,
    },
    rtcp::payload_feedbacks::picture_loss_indication::PictureLossIndication,
    rtp_transceiver::{
        RTCRtpTransceiverInit, rtp_codec::RTPCodecType,
        rtp_transceiver_direction::RTCRtpTransceiverDirection,
    },
    track::{
        track_local::{TrackLocal, track_local_static_rtp::TrackLocalStaticRTP},
        track_remote::TrackRemote,
    },
};

use crate::{
    error::Result,
    signal::{self, ServerMessage},
    track::Track,
};

static MEDIA_ENGINE_MUTEX: Lazy<std::sync::Mutex<()>> = Lazy::new(|| std::sync::Mutex::new(()));

pub struct PeerTrack {
    pub inner: Arc<TrackLocalStaticRTP>,
    pub ssrc: u32,
}

pub struct Peer {
    pub id: u32,
    pub room_id: u32,
    pub conn: RTCPeerConnection,
    pub signal_tx: signal::Sender,
    pub name: Option<String>,
    pub video: Option<PeerTrack>,
    pub audio: Option<PeerTrack>,
    pub pending_candidates: Vec<RTCIceCandidateInit>,
}

pub type OnPeerConnectedHdlrFn =
    Box<dyn (Fn() -> Pin<Box<dyn Future<Output = ()> + Send + 'static>>) + Send + Sync>;

pub type OnPeerCandidateHdlrFn = Box<
    dyn (Fn(RTCIceCandidateInit) -> Pin<Box<dyn Future<Output = ()> + Send + 'static>>)
        + Send
        + Sync,
>;

pub type OnPeerTrackHdlrFn = Box<
    dyn (Fn(Arc<TrackRemote>) -> Pin<Box<dyn Future<Output = ()> + Send + 'static>>) + Send + Sync,
>;

impl Peer {
    pub async fn new(id: u32, room_id: u32, signal_tx: signal::Sender) -> Result<Self> {
        let api = APIBuilder::new()
            .with_media_engine({
                let mut engine = MediaEngine::default();
                let _lock = MEDIA_ENGINE_MUTEX.lock().unwrap();
                engine.register_default_codecs()?;
                engine
            })
            .build();
        let config = RTCConfiguration::default();
        let conn = api.new_peer_connection(config).await?;
        let mut peer = Self {
            id,
            room_id,
            conn,
            signal_tx,
            name: None,
            video: None,
            audio: None,
            pending_candidates: Vec::new(),
        };
        peer.signal_tx.send(ServerMessage::Id(id)).await?;
        peer.debug("new peer");
        Ok(peer)
    }

    fn debug_format(&self, message: &str) -> String {
        format!("[{}][{}] {}", self.room_id, self.id, message)
    }

    fn debug(&self, message: &str) {
        println!("{}", self.debug_format(message));
    }

    pub fn on_track(&self, f: OnPeerTrackHdlrFn) {
        let message = self.debug_format("new remote track");
        self.conn.on_track(Box::new(move |track_remote, _, _| {
            println!("{}", message);
            f(track_remote)
        }));
    }

    pub fn on_connected(&self, f: OnPeerConnectedHdlrFn) {
        let message = self.debug_format("connection established");
        self.conn
            .on_peer_connection_state_change(Box::new(move |state| {
                if state != RTCPeerConnectionState::Connected {
                    return Box::pin(async {});
                }
                println!("{}", message);
                f()
            }));
    }

    pub fn on_candidate(&self, f: OnPeerCandidateHdlrFn) {
        let message = self.debug_format("new local ice candidate");
        self.conn.on_ice_candidate(Box::new(move |candidate| {
            if let Some(candidate) = candidate {
                println!("{}", message);
                f(candidate.to_json().unwrap())
            } else {
                Box::pin(async {})
            }
        }));
    }

    async fn set_remote_description(&mut self, description: RTCSessionDescription) -> Result<()> {
        self.conn.set_remote_description(description).await?;
        for candidate in self.pending_candidates.drain(..) {
            self.conn.add_ice_candidate(candidate).await?;
        }
        Ok(())
    }

    pub fn is_audio_and_video(&self) -> bool {
        self.audio.is_some() && self.video.is_some()
    }

    pub async fn add_recvonly_transceiver(&self, kind: RTPCodecType) -> Result<()> {
        self.conn
            .add_transceiver_from_kind(
                kind,
                Some(RTCRtpTransceiverInit {
                    direction: RTCRtpTransceiverDirection::Recvonly,
                    send_encodings: vec![],
                }),
            )
            .await?;
        self.debug(&format!("recvonly {} transceiver added", kind));
        Ok(())
    }

    pub async fn add_sendonly_transceiver(&self, track: &Arc<TrackLocalStaticRTP>) -> Result<()> {
        self.conn
            .add_transceiver_from_track(
                Arc::clone(track) as Arc<dyn TrackLocal + Send + Sync>,
                Some(RTCRtpTransceiverInit {
                    direction: RTCRtpTransceiverDirection::Sendonly,
                    send_encodings: vec![],
                }),
            )
            .await?;
        self.debug(&format!("sendonly {} transceiver added", track.kind()));
        Ok(())
    }

    pub async fn stop_transceivers(&self, peer_id: u32) -> Result<()> {
        let peer_id_str = peer_id.to_string();
        for transceiver in self.conn.get_transceivers().await {
            if let Some(track) = transceiver.sender().await.track().await {
                if track.id().starts_with(&peer_id_str) {
                    transceiver.stop().await?;
                    self.debug(&format!(
                        "peer {} {} transceiver stopped",
                        peer_id,
                        track.kind()
                    ));
                }
            }
        }
        Ok(())
    }

    pub fn set_name(&mut self, name: String) {
        self.name = Some(name)
    }

    pub fn set_track(&mut self, track: Track) {
        match track.kind {
            RTPCodecType::Audio => self.audio = Some(track.inner),
            RTPCodecType::Video => self.video = Some(track.inner),
            _ => {}
        }
    }

    pub async fn send_message(&mut self, message: ServerMessage) -> Result<()> {
        self.signal_tx.send(message).await
    }

    pub async fn send_pli(&self) -> Result<()> {
        if let Some(video) = &self.video {
            self.conn
                .write_rtcp(&[Box::new(PictureLossIndication {
                    sender_ssrc: 0,
                    media_ssrc: video.ssrc,
                })])
                .await?;
            self.debug("pli sent");
        }
        Ok(())
    }

    pub async fn send_offer(&mut self) -> Result<()> {
        let offer = self.conn.create_offer(None).await?;
        self.conn.set_local_description(offer.clone()).await?;
        self.signal_tx.send(ServerMessage::Offer(offer.sdp)).await?;
        self.debug("offer sent");
        Ok(())
    }

    pub async fn recv_offer(&mut self, sdp: String) -> Result<()> {
        if self.conn.signaling_state() != RTCSignalingState::Stable {
            self.debug("signaling state not stable");
            return Ok(());
        }
        self.debug("offer received");
        self.set_remote_description(RTCSessionDescription::offer(sdp)?)
            .await?;
        let answer = self.conn.create_answer(None).await?;
        self.conn.set_local_description(answer.clone()).await?;
        self.signal_tx
            .send(ServerMessage::Answer(answer.sdp))
            .await?;
        self.debug("answer sent");
        Ok(())
    }

    pub async fn recv_answer(&mut self, sdp: String) -> Result<()> {
        self.debug("answer received");
        self.set_remote_description(RTCSessionDescription::answer(sdp)?)
            .await?;
        Ok(())
    }

    pub async fn add_candidate(&mut self, candidate: RTCIceCandidateInit) -> Result<()> {
        self.debug("new remote ice candidate");
        if self.conn.remote_description().await.is_some() {
            self.conn.add_ice_candidate(candidate).await?;
        } else {
            self.pending_candidates.push(candidate);
        }
        Ok(())
    }

    pub async fn close(&self) -> Result<()> {
        self.conn.close().await?;
        self.debug("connection closed");
        Ok(())
    }
}
