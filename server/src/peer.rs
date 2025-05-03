use webrtc::{
    api::{APIBuilder, media_engine::MediaEngine},
    ice_transport::{ice_candidate::RTCIceCandidateInit, ice_gatherer::OnLocalCandidateHdlrFn},
    peer_connection::{
        OnPeerConnectionStateChangeHdlrFn, RTCPeerConnection, configuration::RTCConfiguration,
        sdp::session_description::RTCSessionDescription,
    },
};

pub struct Peer {
    conn: RTCPeerConnection,
}

impl Peer {
    pub async fn new() -> Self {
        let mut media_engine = MediaEngine::default();
        media_engine.register_default_codecs().unwrap();
        let api = APIBuilder::new().with_media_engine(media_engine).build();
        let config = RTCConfiguration::default();
        let conn = api.new_peer_connection(config).await.unwrap();
        Self { conn }
    }

    pub fn on_connection_state_change(&self, f: OnPeerConnectionStateChangeHdlrFn) {
        self.conn.on_peer_connection_state_change(f);
    }

    pub fn on_ice_candidate(&self, f: OnLocalCandidateHdlrFn) {
        self.conn.on_ice_candidate(f);
    }

    pub async fn add_ice_candidate(&self, candidate: RTCIceCandidateInit) {
        self.conn.add_ice_candidate(candidate).await.unwrap();
    }

    pub async fn set_offer(&self, sdp: String) {
        self.conn
            .set_remote_description(RTCSessionDescription::offer(sdp).unwrap())
            .await
            .unwrap();
    }

    pub async fn create_answer(&self) -> String {
        let answer = self.conn.create_answer(None).await.unwrap();
        self.conn
            .set_local_description(answer.clone())
            .await
            .unwrap();
        return answer.sdp;
    }
}
