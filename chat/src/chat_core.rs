use std::collections::HashMap;
use tokio::sync::mpsc::UnboundedSender;
use uuid::Uuid;

pub enum Frame {
    Enter(String),
    Message(String),
    IllegalFormat(String),
}

impl Frame {
    pub fn match_first_char(text: &str, c: char) -> bool {
        if let Some(c_index) = text.find(c) {
            c_index == 0
        } else {
            false
        }
    }
}

struct Client {
    sender: UnboundedSender<String>,
}

impl Client {
    fn send(&self, message: String) -> Result<(), String> {
        self.sender
            .send(message)
            .map_err(|e| format!("Encountered an error while sending the message {:?}", e))
    }
}

pub struct ChatRooms {
    clients: HashMap<Uuid, Client>,
    rooms: HashMap<Uuid, String>,
    members: HashMap<String, Vec<Uuid>>,
}

impl ChatRooms {
    pub fn new() -> ChatRooms {
        ChatRooms {
            clients: HashMap::new(),
            rooms: HashMap::new(),
            members: HashMap::new(),
        }
    }
    pub fn append_client(&mut self, id: Uuid, tx_ch: UnboundedSender<String>) {
        self.clients.insert(id, Client { sender: tx_ch });
    }
    pub fn handle_message(&mut self, id: &Uuid, frame: Frame) -> Result<(), String> {
        match frame {
            Frame::IllegalFormat(error_msg) => {
                self.destroy_client(&id);
                Result::Err(error_msg)
            }
            Frame::Enter(room) => {
                if let Some(client) = self.clients.get_mut(&id) {
                    let members = self.members.entry(room.to_owned()).or_insert(Vec::new());
                    members.push(id.clone());
                    let room_before = self.rooms.get(&id).to_owned();
                    if let Some(room_before) = room_before {
                        if let Some(members_before) = self.members.get_mut(room_before) {
                            ChatRooms::remove_member(members_before, &id);
                        }
                    }
                    let ret = client.send(format!(
                        "Enter the room {} from {}\r\n",
                        room,
                        room_before.unwrap_or(&String::from(""))
                    ));
                    self.rooms.insert(id.clone(), room.to_owned());
                    ret
                } else {
                    self.destroy_client(&id);
                    Result::Err(format!(
                        "Client must be present mapped to the given id {:?}",
                        id
                    ))
                }
            }
            Frame::Message(message) => {
                if let Some(client) = self.clients.get_mut(&id) {
                    if let Some(current_room) = self.rooms.get(&id) {
                        if let Some(members) = self.members.get(current_room) {
                            for member in members {
                                if member == id {
                                    continue
                                }
                                if let Some(client) = self.clients.get(member) {
                                    match client.send(message.to_owned()) {
                                        Ok(()) => {}
                                        Err(e) => println!("{}", e),
                                    }
                                } else {
                                    println!("The member doesn't exist {:?}", member)
                                }
                            }
                        }
                        Result::Ok(())
                    } else {
                        client.send(String::from("A room should be specified first.\r\n"))
                    }
                } else {
                    self.destroy_client(&id);
                    Result::Err(format!(
                        "Client must be present mapped to the given id {:?}",
                        id
                    ))
                }
            }
        }
    }
    pub fn destroy_client(&mut self, id: &Uuid) {
        self.clients.remove(&id);
        if let Some(room) = self.rooms.remove(&id) {
            if let Some(members_before) = self.members.get_mut(&room) {
                ChatRooms::remove_member(members_before, &id);
            }
        }
    }
    fn remove_member(members: &mut Vec<Uuid>, uuid: &Uuid) {
        let mut index = 0;
        for m in members.iter() {
            if m == uuid {
                members.remove(index);
                return
            }
            index += 1;
        }
    }
}
