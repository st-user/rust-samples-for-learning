use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};
use uuid::Uuid;

use chat::chat_core::{ChatRooms, Frame};

fn parse_frame(data: Vec<u8>) -> Frame {
    let message = match String::from_utf8(data) {
        Ok(s) => s,
        Err(e) => {
            return Frame::IllegalFormat(format!("Illegal UTF-8 bytes {:?}", e));
        }
    };
    if Frame::match_first_char(&message, '@') {
        let room = message.to_string();
        let room = room.trim();
        Frame::Join(room.to_owned())
    } else {
        Frame::Message(message.to_owned())
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let listener = TcpListener::bind("127.0.0.1:8080").await?;
    let chat_rooms = Arc::new(Mutex::new(ChatRooms::new()));
    loop {
        let chat_rooms = Arc::clone(&chat_rooms);
        let (mut socket, _) = listener.accept().await?;
        tokio::spawn(async move {
            let (mut reader, mut writer) = socket.split();
            let mut buf = [0; 1024];
            let (tx_ch, mut rx_ch): (UnboundedSender<String>, UnboundedReceiver<String>) =
                unbounded_channel();
            let my_id = Uuid::new_v4();
            {
                let mut chat_rooms = chat_rooms.lock().unwrap();
                chat_rooms.append_client(my_id.to_owned(), tx_ch);
            }
            let message_on_end: String = loop {
                tokio::select! {
                    ret = reader.read(&mut buf) => {
                        match ret {
                            Ok(n) => {
                                if n == 0 {
                                    let mut chat_rooms = chat_rooms.lock().unwrap();
                                    chat_rooms.destroy_client(&my_id);
                                    break String::from("Connection has been closed")
                                }
                                let message_bytes = Vec::from(&buf[..n]);
                                let mut chat_rooms = chat_rooms.lock().unwrap();

                                if let Err(msg) = chat_rooms.handle_message(&my_id, parse_frame(message_bytes)) {
                                    // The client has already been destroyed in handle_message.
                                    break msg
                                }

                            },
                            Err(e) => {
                                let mut chat_rooms = chat_rooms.lock().unwrap();
                                chat_rooms.destroy_client(&my_id);
                                break format!("Encountered an error while reading bytes from the socket: {:?}", e)
                            }
                        }
                    },
                    Some(msg) = rx_ch.recv() => {
                        match writer.write_all(msg.as_bytes()).await {
                            Ok(()) => println!("Response has been send"),
                            Err(e) => {
                                let mut chat_rooms = chat_rooms.lock().unwrap();
                                chat_rooms.destroy_client(&my_id);
                                break format!("Encountered an error while writing bytes to the socket: {:?}", e)
                            }
                        }
                    }
                }
            };

            println!("{}", message_on_end);
        });
    }
}
