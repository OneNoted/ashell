use std::process::Command;

pub fn execute_command(command: String) {
    tokio::spawn(async move {
        match Command::new("bash")
            .arg("-c")
            .arg(&command)
            .spawn()
        {
            Ok(mut child) => {
                let _ = child.wait();
            }
            Err(e) => log::error!("Failed to execute command {command}: {e}"),
        }
    });
}

pub fn suspend(cmd: String) {
    tokio::spawn(async move {
        match Command::new("bash").arg("-c").arg(&cmd).spawn() {
            Ok(mut child) => {
                let _ = child.wait();
            }
            Err(e) => log::error!("Failed to execute suspend command: {e}"),
        }
    });
}

pub fn hibernate(cmd: String) {
    tokio::spawn(async move {
        match Command::new("bash").arg("-c").arg(&cmd).spawn() {
            Ok(mut child) => {
                let _ = child.wait();
            }
            Err(e) => log::error!("Failed to execute hibernate command: {e}"),
        }
    });
}

pub fn shutdown(cmd: String) {
    tokio::spawn(async move {
        match Command::new("bash").arg("-c").arg(&cmd).spawn() {
            Ok(mut child) => {
                let _ = child.wait();
            }
            Err(e) => log::error!("Failed to execute shutdown command: {e}"),
        }
    });
}

pub fn reboot(cmd: String) {
    tokio::spawn(async move {
        match Command::new("bash").arg("-c").arg(&cmd).spawn() {
            Ok(mut child) => {
                let _ = child.wait();
            }
            Err(e) => log::error!("Failed to execute reboot command: {e}"),
        }
    });
}

pub fn logout(cmd: String) {
    tokio::spawn(async move {
        match Command::new("bash").arg("-c").arg(&cmd).spawn() {
            Ok(mut child) => {
                let _ = child.wait();
            }
            Err(e) => log::error!("Failed to execute logout command: {e}"),
        }
    });
}
