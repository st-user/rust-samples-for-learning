use std::sync::{Arc, Mutex};
use warp::Filter;

use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};
use uuid::Uuid;

use futures::{SinkExt, StreamExt};
use tokio_stream::wrappers::UnboundedReceiverStream;
use warp::ws::Message;

use chat::chat_core::{ChatRooms, Frame};

fn parse_frame(raw_message: Message) -> Frame {
    let message = match raw_message.to_str() {
        Ok(msg) => msg,
        Err(e) => {
            return Frame::IllegalFormat(format!(
                "Illegal message format {:?} (or the connection has been closed)",
                e
            ))
        }
    };
    if Frame::match_first_char(&message, '@') {
        let room = message.to_string();
        let room = room.trim();
        Frame::Enter(room.to_owned())
    } else {
        Frame::Message(message.to_owned())
    }
}

type ChatRoomsHolder = Arc<Mutex<ChatRooms>>;

#[tokio::main]
async fn main() {
    let chat_rooms = Arc::new(Mutex::new(ChatRooms::new()));
    let route = warp::path("chat")
        .and(warp::ws())
        .and(warp::any().map(move || chat_rooms.clone()))
        .map(|ws: warp::ws::Ws, chat_rooms: ChatRoomsHolder| {
            ws.on_upgrade(|websocket| handle_upgrade(websocket, chat_rooms))
        });
    warp::serve(route).run(([127, 0, 0, 1], 8080)).await;
}

async fn handle_upgrade(ws: warp::ws::WebSocket, chat_rooms: ChatRoomsHolder) {
    let (mut ws_tx, mut ws_rx) = ws.split();
    let (tx_ch, rx_ch): (UnboundedSender<String>, UnboundedReceiver<String>) = unbounded_channel();
    let my_id = Uuid::new_v4();
    {
        let mut chat_rooms = chat_rooms.lock().unwrap();
        chat_rooms.append_client(my_id.to_owned(), tx_ch);
    }
    let mut rx_ch = UnboundedReceiverStream::new(rx_ch);
    let message_on_end: String = loop {
        tokio::select! {
            Some(msg) = rx_ch.next() => {
                match ws_tx.send(Message::text(msg)).await {
                    Ok(_) => println!("A message has been send"),
                    Err(e) => {
                        let mut chat_rooms = chat_rooms.lock().unwrap();
                        chat_rooms.destroy_client(&my_id);
                        break format!("Encountered an error while writing a message to the socket: {:?}", e)
                    }
                }
            },
            Some(msg) = ws_rx.next() => {
                match msg {
                    Ok(msg) => {
                        let mut chat_rooms = chat_rooms.lock().unwrap();
                        if let Err(msg) = chat_rooms.handle_message(&my_id, parse_frame(msg)) {
                            // The client has already been destroyed in handle_message.
                            break msg
                        }
                    },
                    Err(e) => {
                        let mut chat_rooms = chat_rooms.lock().unwrap();
                        chat_rooms.destroy_client(&my_id);
                        break format!("Encountered an error while reading a message from the socket: {:?}", e)
                    }
                }
            }
        }
    };
    println!("{}", message_on_end);
}
