extern crate chrono;
extern crate iui;
use super::notification;
use chrono::NaiveDateTime;
use serde_json::json;
use serde_json::Value;
use std::error::Error;
use std::io::{BufRead, BufReader, Write};
use std::process::{ChildStdin, ChildStdout, Command, Stdio};
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering::SeqCst;
use std::sync::mpsc;
use std::sync::mpsc::{Receiver, Sender};
use std::sync::Arc;
use std::thread;
use std::thread::JoinHandle;

#[derive(PartialEq)]
enum MsgType {
    ChannelList,
    ChatMsg,
    ChatMsgList,
    Unknown,
}

pub struct ChatMsg {
    pub utc_timestamp: chrono::NaiveDateTime,
    pub channel: String,
    pub conversation_id: String,
    pub text: String,
}

pub struct Channel {
    pub name: String,
    pub id: String,
    pub unread_msgs: bool,
}

pub struct KeybaseRequest {
    pub msg: Value,
}

pub enum KeybaseReply {
    ChatMsgReply { msg: ChatMsg },
    ChatMsgListReply { msgs: Vec<ChatMsg> },
    ChannelListReply { channels: Vec<Channel> },
}

#[derive(Debug)]
enum KeybaseInternalError {
    IoError,
    ParseError,
    UnknownMessage,
    InvalidMessageFormat,
}

impl From<std::sync::mpsc::RecvError> for KeybaseInternalError {
    fn from(_: std::sync::mpsc::RecvError) -> KeybaseInternalError {
        KeybaseInternalError::IoError
    }
}

impl From<std::sync::mpsc::SendError<KeybaseReply>> for KeybaseInternalError {
    fn from(_: std::sync::mpsc::SendError<KeybaseReply>) -> KeybaseInternalError {
        KeybaseInternalError::IoError
    }
}

impl From<std::io::Error> for KeybaseInternalError {
    fn from(_: std::io::Error) -> KeybaseInternalError {
        KeybaseInternalError::IoError
    }
}

impl From<serde_json::error::Error> for KeybaseInternalError {
    fn from(_: serde_json::error::Error) -> KeybaseInternalError {
        KeybaseInternalError::ParseError
    }
}

impl From<std::num::ParseIntError> for KeybaseInternalError {
    fn from(_: std::num::ParseIntError) -> KeybaseInternalError {
        KeybaseInternalError::ParseError
    }
}

fn safe_json_to_string(v: &Value) -> String {
    match serde_json::to_string_pretty(&v) {
        Ok(stringified) => stringified,
        Err(_) => "[parse error]".to_string(),
    }
}

pub struct Keybase {
    is_running: Arc<AtomicBool>,
    listener_thread: Option<JoinHandle<()>>,
    api_thread: Option<JoinHandle<()>>,
    incoming_tx: Sender<KeybaseReply>,
    incoming_rx: Receiver<KeybaseReply>,
    outgoing_tx: Sender<KeybaseRequest>,
}

impl Drop for Keybase {
    fn drop(&mut self) {
        println!("Destructing Keybase.");
        self.is_running.swap(false, SeqCst);

        if let Some(handle) = self.api_thread.take() {
            println!("Joining API thread back.");
            let empty_msg = KeybaseRequest { msg: json!({}) };
            match self.outgoing_tx.send(empty_msg) {
                Ok(_) => {
                    handle.join().expect("API thread join failed.");
                }
                Err(_) => {
                    println!("Can't join API thread.");
                }
            }
        }

        if let Some(_handle) = self.listener_thread.take() {
            println!("Joining listener thread back.");
            // TODO:
            //handle.join().expect("Listener thread join failed.");
        }
    }
}

impl Keybase {
    pub fn new() -> Self {
        let (outgoing_tx, outgoing_rx): (Sender<KeybaseRequest>, Receiver<KeybaseRequest>) =
            mpsc::channel();
        let (incoming_tx, incoming_rx): (Sender<KeybaseReply>, Receiver<KeybaseReply>) =
            mpsc::channel();
        let mut ret = Keybase {
            is_running: Arc::new(AtomicBool::new(true)),
            listener_thread: None,
            api_thread: None,
            incoming_tx: incoming_tx,
            incoming_rx: incoming_rx,
            outgoing_tx: outgoing_tx,
        };

        ret.listen_new_kb_msgs();
        ret.start_api_loop(outgoing_rx);
        return ret;
    }

    fn get_next_message(
        stdout: &mut BufReader<ChildStdout>,
    ) -> Result<KeybaseReply, KeybaseInternalError> {
        let mut s = String::new();
        stdout.read_line(&mut s)?;

        let parsed = Keybase::parse_json(&s)?;
        let keyb_msg = Keybase::to_keybase_msg(&parsed)?;
        return Ok(keyb_msg);
    }

    fn listen_new_kb_msgs(&mut self) {
        println!("Spawning listener thread");

        let is_running = Arc::clone(&self.is_running);
        let tx = self.incoming_tx.clone();
        self.listener_thread = Some(thread::spawn(move || {
            // keybase chat api-listen
            let process = match Command::new("keybase")
                .arg("chat")
                .arg("api-listen")
                .stdout(Stdio::piped())
                .spawn()
            {
                Err(err) => panic!("Couldn't spawn API listener: {}", err.description()),
                Ok(process) => process,
            };

            let proc_stdout = match process.stdout {
                Some(proc_stdout) => proc_stdout,
                None => panic!("Couldn't map stdout."),
            };

            println!("Starting listen loop.");
            let mut stdout_buf = BufReader::new(proc_stdout);
            loop {
                if is_running.load(SeqCst) == false {
                    break;
                }

                let keyb_msg = match Keybase::get_next_message(&mut stdout_buf) {
                    Err(KeybaseInternalError::IoError) => {
                        // IO-error can't be recovered for now.
                        panic!("Error in listen loop loop.");
                    }
                    Err(_) => continue,
                    Ok(keyb_msg) => keyb_msg,
                };

                match &keyb_msg {
                    KeybaseReply::ChatMsgReply { msg } => {
                        notification::send_desktop_notification(&format!(
                            "Keybase: New message from {}",
                            msg.channel
                        ));
                    }
                    // Ignore other types.
                    _ => (),
                }

                match tx.send(keyb_msg) {
                    Ok(_) => {}
                    Err(err) => {
                        println!("Error sending: {}", err);
                    }
                }
            }

            println!("Closing listener thread.");
        }));
    }

    fn handle_next_call(
        stdin: &mut ChildStdin,
        stdout: &mut BufReader<ChildStdout>,
        rx: &Receiver<KeybaseRequest>,
        tx: &Sender<KeybaseReply>,
    ) -> Result<(), KeybaseInternalError> {
        let new_msg = rx.recv()?;
        let json_str = serde_json::to_string(&new_msg.msg)?;
        stdin.write(json_str.as_bytes())?;

        let mut s = String::new();
        stdout.read_line(&mut s)?;
        let parsed = Keybase::parse_json(&s)?;

        let keyb_msg = Keybase::to_keybase_msg(&parsed)?;
        tx.send(keyb_msg)?;
        Ok(())
    }

    fn start_api_loop(&mut self, outgoing_rx: Receiver<KeybaseRequest>) {
        println!("Spawning input thread");

        let tx = self.incoming_tx.clone();
        let is_running = Arc::clone(&self.is_running);
        self.api_thread = Some(thread::spawn(move || {
            // keybase chat api
            let mut process = match Command::new("keybase")
                .arg("chat")
                .arg("api")
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .spawn()
            {
                Err(why) => panic!("Couldn't spawn keybase comms thread: {}", why.description()),
                Ok(process) => process,
            };

            println!("Starting API msg loop.");
            let mut stdin = match process.stdin.as_mut() {
                Some(stdin) => stdin,
                None => panic!("Couldn't map stdin."),
            };

            let stdout = match process.stdout {
                Some(stdout) => stdout,
                None => panic!("Couldn't map stdout."),
            };

            let mut stdout_buf = BufReader::new(stdout);
            loop {
                if is_running.load(SeqCst) == false {
                    break;
                }

                match Keybase::handle_next_call(&mut stdin, &mut stdout_buf, &outgoing_rx, &tx) {
                    Err(KeybaseInternalError::IoError) => {
                        // IO-error can't be recovered for now.
                        panic!("Error in API loop.");
                    }
                    Err(_) => continue,
                    Ok(()) => continue,
                }
            }

            println!("Closing API thread.");
        }));
    }

    pub fn login(&self) -> Result<(), String> {
        let status = match Command::new("keybase").arg("login").status() {
            Ok(s) => s,
            Err(_) => return Err("Spawning keybase process failed".to_string()),
        };

        if !status.success() {
            return Err("Login failed".to_string());
        }
        Ok(())
    }

    pub fn get_message_receiver(&self) -> &Receiver<KeybaseReply> {
        return &self.incoming_rx;
    }

    pub fn get_message_sender(&self) -> Sender<KeybaseRequest> {
        return self.outgoing_tx.clone();
    }

    pub fn create_msg_req(conversation_id: &str, text: &str) -> KeybaseRequest {
        KeybaseRequest {
            msg: json!({
                "method": "send",
                "params": {
                    "options": {
                        "conversation_id": conversation_id,
                        "message": {"body": text}
                    }
                }
            }),
        }
    }

    pub fn create_read_conversation_req(conversation_id: &str, num_msgs: usize) -> KeybaseRequest {
        KeybaseRequest {
            msg: json!({
                "method": "read",
                "params": {
                    "options": {
                          "conversation_id": conversation_id,
                          "pagination": {
                              "num": num_msgs
                          }
                    }
                }
            }),
        }
    }

    pub fn create_list_channels_req() -> KeybaseRequest {
        KeybaseRequest {
            msg: json!({
                "method": "list"
            }),
        }
    }

    fn parse_json(json_str: &str) -> Result<Value, KeybaseInternalError> {
        match serde_json::from_str(&json_str) {
            Ok(val) => {
                // For debugging.
                // println!("{}", safe_json_to_string(&val));
                Ok(val)
            }
            Err(err) => {
                println!("Parse error: {}", err);
                Err(KeybaseInternalError::ParseError)
            }
        }
    }

    fn parse_chat_msg(v: &Value) -> Result<ChatMsg, KeybaseInternalError> {
        if v["msg"]["content"]["type"] == "text" {
            let ts_unix_epoch = v["msg"]["sent_at"].to_string().parse()?;
            let channel = match v["msg"]["sender"]["username"].as_str() {
                Some(channel) => channel.to_string(),
                None => return Err(KeybaseInternalError::ParseError),
            };
            let text = match v["msg"]["content"]["text"]["body"].as_str() {
                Some(text) => text.trim().to_string(),
                None => return Err(KeybaseInternalError::ParseError),
            };

            let conversation_id = match v["msg"]["conversation_id"].as_str() {
                Some(conv_id) => conv_id.to_string(),
                None => return Err(KeybaseInternalError::ParseError),
            };

            return Ok(ChatMsg {
                utc_timestamp: NaiveDateTime::from_timestamp(ts_unix_epoch, 0),
                channel: channel,
                conversation_id: conversation_id,
                text: text,
            });
        }
        println!("Not a chat msg: {}", safe_json_to_string(&v));
        return Err(KeybaseInternalError::ParseError);
    }

    fn create_chat_msg_reply(v: &Value) -> Result<KeybaseReply, KeybaseInternalError> {
        match Keybase::parse_chat_msg(&v) {
            Ok(chat_msg) => return Ok(KeybaseReply::ChatMsgReply { msg: chat_msg }),
            Err(err) => {
                println!("Not a chat msg: {}.", safe_json_to_string(&v));
                return Err(err);
            }
        }
    }

    fn create_chat_msg_list_reply(v: &Value) -> Result<KeybaseReply, KeybaseInternalError> {
        let messages = match v["result"]["messages"].as_array() {
            Some(messages) => messages,
            None => {
                println!("Not a chat msg list: {}", safe_json_to_string(&v));
                return Err(KeybaseInternalError::ParseError);
            }
        };

        let mut ret: Vec<ChatMsg> = Vec::new();
        for m in messages {
            match Keybase::parse_chat_msg(&m) {
                Ok(chat_msg) => {
                    ret.push(chat_msg);
                }
                Err(_) => {
                    println!("Skipped message: {}", safe_json_to_string(&v));
                }
            }
        }
        return Ok(KeybaseReply::ChatMsgListReply { msgs: ret });
    }

    fn create_channel_list_reply(v: &Value) -> Result<KeybaseReply, KeybaseInternalError> {
        let conversations = match v["result"]["conversations"].as_array() {
            Some(converstations) => converstations,
            None => {
                println!("Not a channel list: {}", safe_json_to_string(&v),);
                return Err(KeybaseInternalError::InvalidMessageFormat);
            }
        };

        let mut ret: Vec<Channel> = Vec::new();
        for c in conversations {
            let name = match c["channel"]["name"].as_str() {
                Some(name) => name.to_string(),
                None => return Err(KeybaseInternalError::ParseError),
            };
            let id = match c["id"].as_str() {
                Some(id) => id.to_string(),
                None => return Err(KeybaseInternalError::ParseError),
            };
            let unread_msgs = match c["unread"].as_bool() {
                Some(unread_msgs) => unread_msgs,
                None => return Err(KeybaseInternalError::ParseError),
            };

            ret.push(Channel {
                name: name,
                id: id,
                unread_msgs: unread_msgs,
            })
        }
        return Ok(KeybaseReply::ChannelListReply { channels: ret });
    }

    fn get_msg_type(v: &Value) -> MsgType {
        if v["type"] == "chat" && v["msg"]["content"]["type"] == "text" {
            return MsgType::ChatMsg;
        } else if v["result"]["messages"].is_array() {
            return MsgType::ChatMsgList;
        } else if v["result"]["conversations"].is_array() {
            return MsgType::ChannelList;
        }
        return MsgType::Unknown;
    }

    fn to_keybase_msg(v: &Value) -> Result<KeybaseReply, KeybaseInternalError> {
        match Keybase::get_msg_type(&v) {
            MsgType::ChatMsg => match Keybase::create_chat_msg_reply(&v) {
                Ok(msg) => {
                    return Ok(msg);
                }
                Err(err) => {
                    return Err(err);
                }
            },
            MsgType::ChatMsgList => match Keybase::create_chat_msg_list_reply(&v) {
                Ok(msg) => {
                    return Ok(msg);
                }
                Err(err) => {
                    return Err(err);
                }
            },
            MsgType::ChannelList => match Keybase::create_channel_list_reply(&v) {
                Ok(msg) => {
                    return Ok(msg);
                }
                Err(err) => {
                    return Err(err);
                }
            },
            MsgType::Unknown => {
                println!("Unknown message: {}", safe_json_to_string(&v));
                return Err(KeybaseInternalError::UnknownMessage);
            }
        }
    }
}
