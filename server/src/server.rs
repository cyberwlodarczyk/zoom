use crate::{
    code, error,
    session::{Session, SessionHdlrFns},
    signal::{self, ServerMessage},
    state::State,
    track,
};
use axum::{
    Json, Router,
    extract::{
        self, Query,
        ws::{WebSocket, WebSocketUpgrade},
    },
    http::StatusCode,
    response::IntoResponse,
    routing::get,
};
use serde_json::json;
use std::{collections::HashMap, sync::Arc};
use tokio::net::TcpListener;

pub struct Server {
    router: Router,
    addr: String,
}

impl Server {
    pub fn new(addr: &str) -> Self {
        let router = Router::new()
            .route("/code", get(code_handler))
            .route("/signal", get(signal_handler))
            .with_state(Arc::new(State::new()));
        Self {
            router,
            addr: addr.into(),
        }
    }

    pub async fn run(self) {
        let listener = TcpListener::bind(self.addr).await.unwrap();
        axum::serve(listener, self.router).await.unwrap();
    }
}

async fn code_handler() -> impl IntoResponse {
    Json(json!({"code": code::generate()}))
}

async fn signal_handler(
    extract::State(state): extract::State<Arc<State>>,
    Query(params): Query<HashMap<String, String>>,
    ws: WebSocketUpgrade,
) -> impl IntoResponse {
    let code = params.get("code");
    if code.is_none() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "missing query parameter 'code'"})),
        )
            .into_response();
    }
    let code = code.unwrap();
    if !code::is_valid(code) {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "invalid query parameter 'code'"})),
        )
            .into_response();
    }
    let code = code.clone();
    ws.on_upgrade(async move |socket| {
        signal_handler_upgrade(state, socket, code).await;
    })
}

async fn signal_handler_upgrade(state: Arc<State>, socket: WebSocket, code: String) {
    let (error_tx, mut error_rx) = error::channel();
    let (signal_tx, mut signal_rx) = signal::channel(socket, error_tx.clone());

    let session = match Session::new(state, code, signal_tx.clone()).await {
        Ok(s) => s,
        Err(e) => {
            println!("{}", e);
            return;
        }
    };

    let session1 = session.clone();
    tokio::spawn(async move {
        while let Some(error) = error_rx.recv().await {
            println!("{}", error);
        }
        if let Err(error1) = session1.leave().await {
            println!("{}", error1);
        }
    });

    let (track_tx, mut track_rx) = track::channel(session.peer_id(), error_tx.clone());

    let session1 = session.clone();
    let signal_tx1 = signal_tx.clone();
    let error_tx1 = error_tx.clone();
    let error_tx2 = error_tx.clone();
    session
        .on(SessionHdlrFns {
            connected: Box::new(move || {
                let session2 = session1.clone();
                let error_tx2 = error_tx1.clone();
                Box::pin(async move {
                    error_tx2.send(session2.handle_connected()).await;
                })
            }),
            candidate: Box::new(move |candidate| {
                let mut signal_tx2 = signal_tx1.clone();
                let error_tx3 = error_tx2.clone();
                Box::pin(async move {
                    error_tx3
                        .send(signal_tx2.send(ServerMessage::Candidate(candidate)))
                        .await;
                })
            }),
            track: Box::new(move |track_remote| {
                track_tx.clone().send(track_remote);
                Box::pin(async {})
            }),
        })
        .await;

    let session1 = session.clone();
    let error_tx1 = error_tx.clone();
    error_tx1.spawn(async move {
        while let Some(message) = signal_rx.recv().await? {
            session1.handle_message(message).await?;
        }
        session1.leave().await?;
        Ok(())
    });

    let session1 = session.clone();
    let error_tx1 = error_tx.clone();
    error_tx1.spawn(async move {
        while let Some(track) = track_rx.recv().await {
            session1.handle_track(track).await?;
        }
        Ok(())
    });
}
