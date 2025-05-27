use std::sync::Arc;

use tokio::sync::Mutex;
use webrtc::rtp_transceiver::rtp_codec::RTPCodecType;

use crate::{
    error::Result,
    peer::{OnPeerCandidateHdlrFn, OnPeerConnectedHdlrFn, OnPeerTrackHdlrFn},
    room::Room,
    signal::{self, PeerMessage, ServerMessage},
    state::State,
    track::Track,
};

pub struct SessionHdlrFns {
    pub connected: OnPeerConnectedHdlrFn,
    pub candidate: OnPeerCandidateHdlrFn,
    pub track: OnPeerTrackHdlrFn,
}

#[derive(Clone)]
pub struct Session {
    state: Arc<State>,
    code: String,
    room: Arc<Mutex<Room>>,
    peer_id: u32,
}

impl Session {
    pub async fn new(state: Arc<State>, code: String, signal_tx: signal::Sender) -> Result<Self> {
        let room = state.get_room(code.clone());
        let mut room_guard = room.lock().await;
        let peer_id = room_guard.add_peer(signal_tx).await?;
        let peer = room_guard.get_peer(peer_id);
        for kind in [RTPCodecType::Video, RTPCodecType::Audio] {
            peer.add_recvonly_transceiver(kind).await?;
        }
        room_guard.add_other_peers_tracks(peer).await?;
        drop(room_guard);
        Ok(Self {
            state,
            code,
            room,
            peer_id,
        })
    }

    pub fn peer_id(&self) -> u32 {
        self.peer_id
    }

    pub async fn on(&self, fns: SessionHdlrFns) {
        let room_guard = self.room.lock().await;
        let peer = room_guard.get_peer(self.peer_id);
        peer.on_connected(fns.connected);
        peer.on_candidate(fns.candidate);
        peer.on_track(fns.track);
    }

    pub async fn handle_connected(&self) -> Result<()> {
        let room_guard = self.room.lock().await;
        let message = ServerMessage::Peers(room_guard.get_server_message_peers(self.peer_id));
        drop(room_guard);
        let mut room_guard = self.room.lock().await;
        let peer = room_guard.get_peer_mut(self.peer_id);
        peer.send_offer().await?;
        peer.send_message(message).await?;
        if let Some(name) = peer.name.clone() {
            room_guard.send_joined_peer(self.peer_id, name).await?;
        }
        Ok(())
    }

    pub async fn handle_message(&self, message: PeerMessage) -> Result<()> {
        let mut room_guard = self.room.lock().await;
        let peer = room_guard.get_peer_mut(self.peer_id);
        match message {
            PeerMessage::Offer(sdp) => {
                peer.recv_offer(sdp).await?;
            }
            PeerMessage::Answer(sdp) => {
                peer.recv_answer(sdp).await?;
            }
            PeerMessage::Candidate(candidate) => {
                peer.add_candidate(candidate).await?;
            }
            PeerMessage::Name(name) => {
                peer.set_name(name);
            }
            PeerMessage::Pli(id) => {
                room_guard.send_pli(id).await?;
            }
        }
        Ok(())
    }

    pub async fn handle_track(&self, track: Track) -> Result<()> {
        let mut room_guard = self.room.lock().await;
        let peer = room_guard.get_peer_mut(self.peer_id);
        let track_local = Arc::clone(&track.inner.inner);
        peer.set_track(track);
        let send_offer = peer.is_audio_and_video();
        room_guard
            .add_peer_track_to_others(self.peer_id, track_local, send_offer)
            .await?;
        Ok(())
    }

    pub async fn leave(&self) -> Result<()> {
        let mut room_guard = self.room.lock().await;
        let left = room_guard.handle_peer_leave(self.peer_id).await?;
        if left && room_guard.peers.len() == 0 {
            self.state.remove_room(&self.code);
        }
        Ok(())
    }
}
