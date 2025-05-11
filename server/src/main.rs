use crate::{
    signal::{PeerMessage, ServerMessage},
    state::ServerState,
};
use axum::{
    Router,
    extract::{
        State,
        ws::{WebSocket, WebSocketUpgrade},
    },
    response::Response,
    routing::get,
};
use once_cell::sync::Lazy;
use std::sync::{Arc, Mutex};
use tokio::net::TcpListener;
use tokio::sync::{Mutex as TokioMutex, mpsc};
use webrtc::{
    api::{APIBuilder, media_engine::MediaEngine},
    peer_connection::{
        RTCPeerConnection, configuration::RTCConfiguration,
        peer_connection_state::RTCPeerConnectionState,
        sdp::session_description::RTCSessionDescription,
    },
    rtcp::payload_feedbacks::picture_loss_indication::PictureLossIndication,
    rtp_transceiver::{
        RTCRtpTransceiverInit, rtp_codec::RTPCodecType,
        rtp_transceiver_direction::RTCRtpTransceiverDirection,
    },
    track::track_local::{
        TrackLocal, TrackLocalWriter, track_local_static_rtp::TrackLocalStaticRTP,
    },
};

mod signal;
mod state;

static MEDIA_ENGINE_MUTEX: Lazy<Mutex<()>> = Lazy::new(|| Mutex::new(()));

fn create_media_engine() -> MediaEngine {
    let mut engine = MediaEngine::default();
    let _lock = MEDIA_ENGINE_MUTEX.lock().unwrap();
    engine.register_default_codecs().unwrap();
    return engine;
}

async fn create_peer_connection() -> RTCPeerConnection {
    let api = APIBuilder::new()
        .with_media_engine(create_media_engine())
        .build();
    let config = RTCConfiguration::default();
    return api.new_peer_connection(config).await.unwrap();
}

#[tokio::main]
async fn main() {
    let router = Router::new()
        .route("/signal", get(signal_handler))
        .with_state(Arc::new(TokioMutex::new(ServerState::new())));
    let listener = TcpListener::bind("0.0.0.0:3000").await.unwrap();
    axum::serve(listener, router).await.unwrap();
}

async fn signal_handler(
    State(state): State<Arc<TokioMutex<ServerState>>>,
    ws: WebSocketUpgrade,
) -> Response {
    ws.on_upgrade(async move |socket| {
        signal_handler_upgrade(state, socket).await;
    })
}

async fn signal_handler_upgrade(state: Arc<TokioMutex<ServerState>>, socket: WebSocket) {
    let (signal_tx, mut signal_rx) = signal::channel(socket);
    let (local_track_tx, mut local_track_rx) = mpsc::channel::<Arc<TrackLocalStaticRTP>>(1);
    let peer_conn = create_peer_connection().await;

    let mut state_lock = state.lock().await;
    let id = state_lock.add_peer(peer_conn);
    let peer = state_lock.get_peer(id);

    println!("[{id}] new peer");

    println!("[{id}] adding recvonly transceiver");
    peer.conn
        .add_transceiver_from_kind(
            RTPCodecType::Video,
            Some(RTCRtpTransceiverInit {
                direction: RTCRtpTransceiverDirection::Recvonly,
                send_encodings: vec![],
            }),
        )
        .await
        .unwrap();

    for other in &state_lock.peers {
        if other.id == id {
            continue;
        }
        println!("[{}] adding peer {} track", id, other.id);
        if let Some(other_local_track) = &other.local_track {
            peer.conn
                .add_track(Arc::clone(other_local_track) as Arc<dyn TrackLocal + Send + Sync>)
                .await
                .unwrap();
        }
    }

    peer.conn
        .on_peer_connection_state_change(Box::new(move |state| {
            if state == RTCPeerConnectionState::Connected {
                println!("[{id}] connection established");
            }
            Box::pin(async {})
        }));

    let signal_tx1 = signal_tx.clone();
    let state2 = Arc::clone(&state);
    peer.conn.on_negotiation_needed(Box::new(move || {
        println!("[{id}] negotiation needed");
        let mut signal_tx2 = signal_tx1.clone();
        let state3 = Arc::clone(&state2);
        Box::pin(async move {
            let state_lock = state3.lock().await;
            let peer = state_lock.get_peer(id);
            if peer.conn.connection_state() != RTCPeerConnectionState::Connected {
                return;
            }
            let offer = peer.conn.create_offer(None).await.unwrap();
            peer.conn
                .set_local_description(offer.clone())
                .await
                .unwrap();
            signal_tx2
                .send(ServerMessage::Offer { sdp: offer.sdp })
                .await;
            println!("[{id}] offer sent")
        })
    }));

    let signal_tx1 = signal_tx.clone();
    peer.conn.on_ice_candidate(Box::new(move |candidate| {
        println!("[{id}] new local ice candidate");
        let mut signal_tx2 = signal_tx1.clone();
        Box::pin(async move {
            if let Some(candidate) = candidate {
                signal_tx2
                    .send(ServerMessage::Candidate(candidate.to_json().unwrap()))
                    .await;
            }
        })
    }));

    let local_track_tx1 = Arc::new(local_track_tx);
    let state1 = Arc::clone(&state);
    peer.conn.on_track(Box::new(move |remote_track, _, _| {
        println!("[{id}] new remote track");
        let media_ssrc = remote_track.ssrc();
        let local_track_tx2 = Arc::clone(&local_track_tx1);
        let state2 = Arc::clone(&state1);
        tokio::spawn(async move {
            let local_track = Arc::new(TrackLocalStaticRTP::new(
                remote_track.codec().capability,
                id.to_string(),
                remote_track.stream_id(),
            ));
            local_track_tx2
                .send(Arc::clone(&local_track))
                .await
                .unwrap();
            while let Ok((rtp, _)) = remote_track.read_rtp().await {
                local_track.write_rtp(&rtp).await.unwrap();
            }
        });
        Box::pin(async move {
            let mut state_lock = state2.lock().await;
            let peer = state_lock.get_peer_mut(id);
            peer.media_ssrc = Some(media_ssrc);
        })
    }));

    drop(state_lock);

    let mut signal_tx1 = signal_tx.clone();
    let state1 = Arc::clone(&state);
    tokio::spawn(async move {
        while let Some(msg) = signal_rx.recv().await {
            let mut state_lock = state1.lock().await;
            let peer = state_lock.get_peer_mut(id);
            match msg {
                PeerMessage::Offer { sdp } => {
                    println!("[{id}] offer received");
                    peer.conn
                        .set_remote_description(RTCSessionDescription::offer(sdp).unwrap())
                        .await
                        .unwrap();
                    let answer = peer.conn.create_answer(None).await.unwrap();
                    peer.conn
                        .set_local_description(answer.clone())
                        .await
                        .unwrap();
                    signal_tx1
                        .send(ServerMessage::Answer { sdp: answer.sdp })
                        .await;
                    println!("[{id}] answer sent");
                }
                PeerMessage::Answer { sdp } => {
                    println!("[{id}] answer received");
                    peer.conn
                        .set_remote_description(RTCSessionDescription::answer(sdp).unwrap())
                        .await
                        .unwrap();
                }
                PeerMessage::Candidate(candidate) => {
                    println!("[{id}] new remote ice candidate");
                    peer.conn.add_ice_candidate(candidate).await.unwrap();
                }
                PeerMessage::Name(name) => peer.name = Some(name),
                PeerMessage::Pli(_) => {
                    println!("[{id}] pli request received");
                    if let Some(media_ssrc) = peer.media_ssrc {
                        peer.conn
                            .write_rtcp(&[Box::new(PictureLossIndication {
                                sender_ssrc: 0,
                                media_ssrc,
                            })])
                            .await
                            .unwrap();
                    }
                }
            }
        }
    });

    if let Some(local_track) = local_track_rx.recv().await {
        let mut state_lock = state.lock().await;
        let peer = state_lock.get_peer_mut(id);
        peer.local_track = Some(Arc::clone(&local_track));
        for other in &state_lock.peers {
            if other.id == id {
                continue;
            }
            println!("[{}] adding peer {} track", other.id, id);
            other
                .conn
                .add_track(Arc::clone(&local_track) as Arc<dyn TrackLocal + Send + Sync>)
                .await
                .unwrap();
        }
    }
}
