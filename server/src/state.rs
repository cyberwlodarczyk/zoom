use std::sync::Arc;

use webrtc::{
    peer_connection::RTCPeerConnection,
    track::track_local::track_local_static_rtp::TrackLocalStaticRTP,
};

pub struct MediaState {
    pub track: Arc<TrackLocalStaticRTP>,
    pub ssrc: u32,
}

pub struct PeerState {
    pub id: u32,
    pub conn: RTCPeerConnection,
    pub name: Option<String>,
    pub video: Option<MediaState>,
    pub audio: Option<MediaState>,
}

pub struct ServerState {
    next_peer_id: u32,
    pub peers: Vec<PeerState>,
}

impl ServerState {
    pub fn new() -> Self {
        Self {
            next_peer_id: 1,
            peers: Vec::new(),
        }
    }

    pub fn get_peer(&self, id: u32) -> &PeerState {
        let index = self.peers.iter().position(|peer| peer.id == id).unwrap();
        &self.peers[index]
    }

    pub fn get_peer_mut(&mut self, id: u32) -> &mut PeerState {
        let index = self.peers.iter().position(|peer| peer.id == id).unwrap();
        &mut self.peers[index]
    }

    pub fn add_peer(&mut self, conn: RTCPeerConnection) -> u32 {
        let id = self.next_peer_id;
        self.peers.push(PeerState {
            id,
            conn,
            name: None,
            video: None,
            audio: None,
        });
        self.next_peer_id += 1;
        id
    }
}
