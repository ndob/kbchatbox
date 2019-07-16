use std::process::Command;

pub fn send_desktop_notification(msg: &str) {
    let ret_val = Command::new("notify-send")
        .arg(msg)
        .arg("-i")
        .arg("mail-read")
        .status();
    println!(
        "Notification sent: {}",
        if ret_val.is_ok() && ret_val.unwrap().success() {
            "success"
        } else {
            "failed"
        }
    );
}
