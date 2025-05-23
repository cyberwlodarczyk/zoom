use std::sync::{
    Arc,
    atomic::{AtomicU32, Ordering},
};

use webrtc::{
    peer_connection::peer_connection_state::RTCPeerConnectionState,
    track::track_local::track_local_static_rtp::TrackLocalStaticRTP,
};

use crate::{
    peer::Peer,
    signal::{self, ServerMessage, ServerMessagePeer},
};

pub struct Room {
    next_peer_id: Arc<AtomicU32>,
    pub id: u32,
    pub peers: Vec<Peer>,
}

impl Room {
    pub fn new(id: u32, next_peer_id: Arc<AtomicU32>) -> Self {
        Self {
            id,
            next_peer_id,
            peers: Vec::new(),
        }
    }

    fn get_peer_index(&self, id: u32) -> usize {
        self.peers.iter().position(|peer| peer.id == id).unwrap()
    }

    pub fn get_peer(&self, id: u32) -> &Peer {
        let index = self.get_peer_index(id);
        &self.peers[index]
    }

    pub fn get_peer_mut(&mut self, id: u32) -> &mut Peer {
        let index = self.get_peer_index(id);
        &mut self.peers[index]
    }

    pub async fn add_peer(&mut self, signal_tx: signal::Sender) -> u32 {
        let id = self.next_peer_id.fetch_add(1, Ordering::Relaxed);
        self.peers.push(Peer::new(id, self.id, signal_tx).await);
        id
    }

    pub fn remove_peer(&mut self, id: u32) -> Peer {
        self.peers.swap_remove(self.get_peer_index(id))
    }

    pub async fn add_other_peers_tracks(&self, peer: &Peer) {
        for other in &self.peers {
            if other.id == peer.id {
                continue;
            }
            for track in [&other.video, &other.audio] {
                if let Some(track) = track {
                    peer.add_sendonly_transceiver(&track.inner).await;
                }
            }
        }
    }

    pub fn get_server_message_peers(&self, for_peer_id: u32) -> Vec<ServerMessagePeer> {
        self.peers
            .iter()
            .filter(|p| {
                p.conn.connection_state() == RTCPeerConnectionState::Connected
                    && p.id != for_peer_id
            })
            .map(|p| ServerMessagePeer {
                id: p.id,
                name: p.name.clone().unwrap(),
            })
            .collect()
    }

    pub async fn send_joined_peer(&mut self, id: u32, name: String) {
        for other in &mut self.peers {
            if other.id == id {
                continue;
            }
            other
                .send_message(ServerMessage::PeerJoined(ServerMessagePeer {
                    id,
                    name: name.clone(),
                }))
                .await;
        }
    }

    pub async fn handle_peer_leave(&mut self, id: u32) {
        let peer = self.remove_peer(id);
        peer.close().await;
        for other in &mut self.peers {
            other.send_message(ServerMessage::PeerLeft(id)).await;
            other.stop_transceivers(id).await;
            other.send_offer().await;
        }
    }

    pub async fn add_peer_track_to_others(
        &mut self,
        peer_id: u32,
        track: Arc<TrackLocalStaticRTP>,
        send_offer: bool,
    ) {
        for other in &mut self.peers {
            if other.id == peer_id {
                continue;
            }
            other.add_sendonly_transceiver(&track).await;
            if send_offer {
                other.send_offer().await;
            }
        }
    }

    pub async fn send_pli(&self, peer_id: u32) {
        for peer in &self.peers {
            if peer.id == peer_id {
                peer.send_pli().await;
            }
        }
    }
}
