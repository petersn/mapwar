use std::{
  collections::{HashMap, HashSet},
  sync::{atomic::AtomicBool, Arc},
  thread,
};

use anyhow::Error;
use futures_util::{SinkExt, StreamExt};
use mapwar::game_state::GameAction;
use serde::{Deserialize, Serialize};
use signal_hook::{consts::SIGTERM, iterator::Signals};
use tokio::sync::{mpsc, RwLock};
use warp::{ws, Filter};

static IS_SHUTTING_DOWN: AtomicBool = AtomicBool::new(false);

type ConnectionId = usize;

struct Game {}

#[derive(Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase", tag = "kind")]
#[ts(export)]
enum WebSocketRequest<'a> {
  Ping,
  JoinLobby,
  LeaveLobby,
  TakeAction {
    game_token: &'a str,
    action:     GameAction,
  },
}

#[derive(Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase", tag = "kind")]
#[ts(export)]
enum WebSocketResponse<'a> {
  Pong,
  GameStarting { game_token: &'a str },
}

struct ConnectionState {
  connection_id:     ConnectionId,
  wakeup_channel_rx: mpsc::Receiver<ConnectionMessage>,
  wakeup_channel_tx: mpsc::Sender<ConnectionMessage>,
}

impl ConnectionState {
  fn new() -> Self {
    let (wakeup_channel_tx, wakeup_channel_rx) = mpsc::channel(8);
    Self {
      connection_id: 0,
      wakeup_channel_rx,
      wakeup_channel_tx,
    }
  }

  async fn send_response<'a>(
    tx: &mut futures_util::stream::SplitSink<ws::WebSocket, ws::Message>,
    response: WebSocketResponse<'a>,
  ) -> Result<(), Error> {
    let message = warp::ws::Message::text(serde_json::to_string(&response).unwrap());
    tx.send(message).await.map_err(|e| e.into())
  }

  async fn handle_message(
    &mut self,
    text: &str,
    tx: &mut futures_util::stream::SplitSink<ws::WebSocket, ws::Message>,
  ) -> Result<(), Error> {
    let request: WebSocketRequest = serde_json::from_str(text)?;
    match request {
      WebSocketRequest::Ping => {
        Self::send_response(tx, WebSocketResponse::Pong).await?;
      }
      WebSocketRequest::JoinLobby => {
        println!("Joining lobby");
      }
      WebSocketRequest::LeaveLobby => {
        println!("Leaving lobby");
      }
      WebSocketRequest::TakeAction { game_token, action } => {
        println!("Taking action: {:?}", action);
      }
    }
    Ok(())
  }

  async fn main_loop(&mut self, ws: ws::WebSocket, global_state: &GlobalState) {
    let (mut tx, mut rx) = ws.split();
    loop {
      tokio::select! {
        // Handle messages from the client.
        ws_message = rx.next() => {
          match ws_message {
            Some(Ok(msg)) => {
              if let Ok(text) = msg.to_str() {
                if let Err(err) = self.handle_message(text, &mut tx).await {
                  println!("Error handling message: {}", err);
                  break;
                }
              } else if msg.is_close() {
                println!("Received close message: {:?}", msg);
                break;
              } else {
                println!("Received non-text message: {:?}", msg);
                break;
              }
              println!("got message: {:?}", msg);
            }
            Some(Err(err)) => {
              println!("Error reading from websocket: {}", err);
              break;
            }
            None => {
              println!("Websocket closed");
              break;
            }
          }
        }

        // Handle a message from another thread.
        connection_message = self.wakeup_channel_rx.recv() => {
          match connection_message {
            Some(ConnectionMessage::Sunset) => {
              println!("Sunset");
              break;
            }
            None => {
              println!("Websocket closed");
              break;
            }
          }
        }
      }
    }
  }
}

impl GlobalState {
  fn new() -> Self {
    Self {
      connections: RwLock::new(HashMap::new()),
      main_lobby:  RwLock::new(HashSet::new()),
      games:       RwLock::new(HashMap::new()),
    }
  }

  async fn lobby_loop(&self) {
    loop {
      if IS_SHUTTING_DOWN.load(std::sync::atomic::Ordering::Relaxed) {
        break;
      }

      // let mut connections = self.connections.write().await;
      // let mut messages = Vec::new();
      // for connection in connections.iter() {
      //   messages.push(ConnectionMessage::LobbyUpdate {
      //     lobby_state: "TODO".to_string(),
      //   });
      // }
      // for message in messages {
      //   for connection in connections.iter() {
      //     connection.send(message.clone()).await;
      //   }
      // }
      // drop(connections);

      tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    }
  }

  fn sunset_lobby(&self) {}
}

enum ConnectionMessage {
  Sunset,
}

struct ConnectionEntry {
  pub notification_channel: mpsc::Sender<ConnectionMessage>,
}

struct GlobalState {
  connections: RwLock<HashMap<ConnectionId, Arc<ConnectionEntry>>>,
  main_lobby:  RwLock<HashSet<ConnectionId>>,
  games:       RwLock<HashMap<String, Arc<Game>>>,
}

async fn user_connected(ws: ws::WebSocket, global_state: &GlobalState) {
  let mut connection_state = ConnectionState::new();
  let connection_entry = Arc::new(ConnectionEntry {
    notification_channel: connection_state.wakeup_channel_tx.clone(),
  });

  // Add us to the global connections list.
  global_state
    .connections
    .write()
    .await
    .insert(connection_state.connection_id, connection_entry.clone());

  let _: () = connection_state.main_loop(ws, global_state).await;

  // Remove us from the global connections list.
  global_state.connections.write().await.remove(&connection_state.connection_id);
}

#[tokio::main]
async fn main() -> Result<(), Error> {
  dotenv::dotenv().ok();

  let global_state: &'static GlobalState = Box::leak(Box::new(GlobalState::new()));
  let warp_global_state = warp::any().map(move || global_state);

  tokio::spawn(global_state.lobby_loop());

  // Handle SIGTERM, which is sent by Kubernetes when it wants to shut down the pod.
  let mut signals = Signals::new(&[SIGTERM]).unwrap();
  thread::spawn(move || {
    for sig in signals.forever() {
      match sig {
        SIGTERM => {
          IS_SHUTTING_DOWN.store(true, std::sync::atomic::Ordering::Relaxed);
          global_state.sunset_lobby();
          println!("SIGTERM received, shutting down");
          std::process::exit(0);
        }
        _ => unreachable!(),
      }
    }
  });

  //let game_data = game_state::load_game_data();
  //println!("game data: {:?}", game_data);
  //return Ok(());

  //let (mut tx, mut rx) = ws.split();

  // FIXME: Only allow sane origins here!
  let cors = warp::cors()
    .allow_any_origin()
    .allow_methods(&[warp::http::Method::GET, warp::http::Method::POST])
    .allow_headers(vec![
      "User-Agent",
      "Sec-Fetch-Mode",
      "Referer",
      "Origin",
      "Access-Control-Request-Method",
      "Access-Control-Request-Headers",
      "Content-Type",
      "X-Requested-With",
    ]);

  let ws_endpoint = warp::path!("api" / "game-connection")
    .and(warp::ws())
    .and(warp_global_state)
    .map(|ws: warp::ws::Ws, gs: &'static GlobalState| {
      ws.on_upgrade(move |socket| user_connected(socket, gs))
    });

  println!("Starting server");
  warp::serve(ws_endpoint.with(cors)).run(([127, 0, 0, 1], 12001)).await;

  Ok(())
}
