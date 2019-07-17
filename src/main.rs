mod keybase;
mod notification;
mod textbuffer;

extern crate chrono;
extern crate iui;

use chrono::{Local, TimeZone};
use iui::controls::*;
use iui::prelude::*;
use keybase::{Channel, ChatMsg, Keybase, KeybaseReply, KeybaseRequest};
use std::sync::mpsc::{Sender, TryRecvError};
use std::sync::{Arc, Mutex};
use textbuffer::TextBuffer;

type ThreadSafeString = std::sync::Arc<std::sync::Mutex<std::string::String>>;

const TEXTBUF_WIDTH: usize = 100;
const TEXTBUF_HEIGHT: usize = 15;

fn format_chat_msg(msg: &keybase::ChatMsg) -> String {
    let ts = Local.from_utc_datetime(&msg.utc_timestamp);
    return format!("{} - {}: {}", ts.format("%F %T"), msg.channel, msg.text);
}

fn safe_send(tx: &Sender<KeybaseRequest>, req: KeybaseRequest) {
    match tx.send(req) {
        Ok(_) => {}
        Err(err) => {
            println!("Error sending: {}", err);
        }
    }
}

fn handle_chat_msg(
    msg: &ChatMsg,
    current_conversation_id: &ThreadSafeString,
    text_buf: &mut TextBuffer,
    label: &mut Label,
    ui: &UI,
) {
    // Only append if the msg is for the currently opened channel.
    let cur_chat = current_conversation_id.lock().unwrap();
    if msg.conversation_id == *cur_chat {
        let formatted = format_chat_msg(&msg);
        text_buf.append(&formatted);
        label.set_text(&ui, &text_buf.get_newest_formatted());
    }
}

fn handle_chat_msg_list(
    msg_list: &Vec<ChatMsg>,
    text_buf: &mut TextBuffer,
    label: &mut Label,
    ui: &UI,
) {
    text_buf.clear();
    for msg in msg_list.iter().rev() {
        let formatted = format_chat_msg(&msg);
        text_buf.append(&formatted);
        label.set_text(&ui, &text_buf.get_newest_formatted());
    }
}

fn handle_channel_list(
    channel_list: &Vec<Channel>,
    current_conversation_id: &ThreadSafeString,
    sender: &Sender<KeybaseRequest>,
    conversations_vbox: &mut VerticalBox,
    ui: &UI,
) {
    // TODO: Implement refresh. This only works once currently.
    for chan in channel_list {
        // Create a button for each conversation.
        let mut button = Button::new(&ui, &chan.name);
        let channel_id = chan.id.clone();
        button.on_clicked(&ui, {
            let current_conversation_id = Arc::clone(&current_conversation_id);
            let sender = sender.clone();
            move |_btn| {
                let mut locked = current_conversation_id.lock().unwrap();
                *locked = channel_id.clone();
                let req = Keybase::create_read_conversation_req(&channel_id, TEXTBUF_HEIGHT);
                safe_send(&sender, req);
            }
        });
        conversations_vbox.append(&ui, button, LayoutStrategy::Compact);
    }
}

fn main() {
    let current_conversation_id = ThreadSafeString::new(Mutex::new(String::new()));
    let kb = Keybase::new();
    let req = Keybase::create_list_channels_req();
    let sender = kb.get_message_sender();
    safe_send(&sender, req);

    let ui = UI::init().expect("Libui init failed.");
    let mut win = Window::new(&ui, "kbchatbox", 640, 480, WindowType::HasMenubar);

    let mut grid = LayoutGrid::new(&ui);
    grid.set_padded(&ui, true);

    // Create space for conversation buttons (left).
    let conversations_vbox = VerticalBox::new(&ui);
    let mut conversations_group = Group::new(&ui, "Conversations");

    conversations_group.set_child(&ui, conversations_vbox.clone());
    grid.append(
        &ui,
        conversations_group.clone(),
        0,
        0,
        1,
        1,
        GridExpand::Neither,
        GridAlignment::Fill,
        GridAlignment::Fill,
    );

    // Create the chat view (right).
    let mut chat_vbox = VerticalBox::new(&ui);
    chat_vbox.set_padded(&ui, true);

    let mut text_buf = TextBuffer::new(TEXTBUF_WIDTH, TEXTBUF_HEIGHT);
    text_buf.append("<--- Click to select a channel.");

    let label = Label::new(&ui, &text_buf.get_newest_formatted());
    chat_vbox.append(&ui, label.clone(), LayoutStrategy::Compact);
    grid.append(
        &ui,
        chat_vbox.clone(),
        1,
        0,
        1,
        1,
        GridExpand::Vertical,
        GridAlignment::Fill,
        GridAlignment::Fill,
    );

    // Create the text entry.
    let mut entry = MultilineEntry::new(&ui);
    entry.on_changed(&ui, {
        let ui = ui.clone();
        let mut entry = entry.clone();
        let current_conversation_id = Arc::clone(&current_conversation_id);
        let sender = sender.clone();
        move |val| {
            let mut newline_found = false;
            for c in val.chars() {
                if c == '\n' {
                    newline_found = true;
                }
            }

            if newline_found {
                entry.set_value(&ui, "");
                let locked = current_conversation_id.lock().unwrap();
                let req = Keybase::create_msg_req(&locked, &val.trim());
                safe_send(&sender, req);
            }
        }
    });
    grid.append(
        &ui,
        entry.clone(),
        1,
        1,
        1,
        1,
        GridExpand::Horizontal,
        GridAlignment::Fill,
        GridAlignment::Fill,
    );

    win.set_child(&ui, grid);
    win.show(&ui);

    let mut event_loop = ui.event_loop();
    event_loop.on_tick(&ui, {
        let ui = ui.clone();
        let mut label = label.clone();
        let mut conversations_vbox = conversations_vbox.clone();
        let sender = sender.clone();
        move || {
            let new_messages_rx = kb.get_message_receiver();
            let res = new_messages_rx.try_recv();
            match res {
                Ok(reply) => match reply {
                    KeybaseReply::ChatMsgReply { msg } => handle_chat_msg(
                        &msg,
                        &current_conversation_id,
                        &mut text_buf,
                        &mut label,
                        &ui,
                    ),
                    KeybaseReply::ChatMsgListReply { msgs } => {
                        handle_chat_msg_list(&msgs, &mut text_buf, &mut label, &ui);
                    }
                    KeybaseReply::ChannelListReply { channels } => {
                        handle_channel_list(
                            &channels,
                            &current_conversation_id,
                            &sender,
                            &mut conversations_vbox,
                            &ui,
                        );
                    }
                },
                Err(error) => match error {
                    TryRecvError::Disconnected => {
                        panic!("Msg recv error");
                    }
                    _ => {}
                },
            }
        }
    });
    event_loop.run(&ui);
}
