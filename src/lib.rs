use std::fs::File;
use std::io::Read;
use std::os::fd::FromRawFd;
use std::os::unix::io::IntoRawFd;
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::process::Command;
use std::process::Stdio;

use ssh2::Session;
pub struct CloudflareSsh {
    session: Session,
}

pub fn bootstrap(app_name: &str, remote_username: &str) -> Result<(), Box<dyn std::error::Error>> {
    let (our_socket, proxy_socket) = UnixStream::pair()?;
    let proxy_fd = proxy_socket.into_raw_fd();
    let stdout_fd = unsafe { libc::dup(proxy_fd) };
    Command::new("cloudflared")
        .args(["access", "ssh", "--hostname", "ssh.gojoe.dev"])
        .stdin(unsafe { Stdio::from_raw_fd(proxy_fd) })
        .stdout(unsafe { Stdio::from_raw_fd(stdout_fd) })
        .spawn()?;

    let mut session = Session::new()?;
    session.set_tcp_stream(our_socket);
    session.handshake()?;
    session.userauth_agent("deepwater")?;

    let sudoers_content = format!(
        "{remote_username} ALL=(ALL) NOPASSWD: /bin/mkdir -p /opt/{app_name}-green
         {remote_username} ALL=(ALL) NOPASSWD: /bin/mkdir -p /opt/{app_name}-blue
         {remote_username} ALL=(ALL) NOPASSWD: /bin/chown -R {remote_username}\\:{remote_username}/opt/{app_name}
         {remote_username} ALL=(ALL) NOPASSWD: /bin/chown -R {remote_username}\\:{remote_username}/etc/systemd/system
         {remote_username} ALL=(ALL) NOPASSWD: /usr/bin/tee /etc/systemd/system/{app_name}-green.service
         {remote_username} ALL=(ALL) NOPASSWD: /usr/bin/tee /etc/systemd/system/{app_name}-blue.service
         {remote_username} ALL=(ALL) NOPASSWD: /usr/bin/systemctl daemon-reload
         {remote_username} ALL=(ALL) NOPASSWD: /usr/bin/systemctl start {app_name}-green
         {remote_username} ALL=(ALL) NOPASSWD: /usr/bin/systemctl start {app_name}-blue
         {remote_username} ALL=(ALL) NOPASSWD: /usr/bin/systemctl stop {app_name}-green
         {remote_username} ALL=(ALL) NOPASSWD: /usr/bin/systemctl stop {app_name}-blue
         {remote_username} ALL=(ALL) NOPASSWD: /usr/bin/systemctl restart {app_name}-green\n
         {remote_username} ALL=(ALL) NOPASSWD: /usr/bin/systemctl restart {app_name}-blue\n"
    );

    let mut channel = session.channel_session()?;
    channel.exec(&format!(
        "echo '{}' > /etc/sudoers.d/{} && chmod 440 /etc/sudoers.d/{} && visudo -c",
        sudoers_content, app_name, app_name
    ))?;

    // Wait for command to complete and check output
    let mut output = String::new();
    channel.read_to_string(&mut output)?;
    channel.wait_close()?;
    println!("bootstrap: {}", output);

    if channel.exit_status()? != 0 {
        return Err("Failed to setup sudoers file".into());
    }

    Ok(())
}

impl CloudflareSsh {
    pub fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let (our_socket, proxy_socket) = UnixStream::pair()?;
        let proxy_fd = proxy_socket.into_raw_fd();
        let stdout_fd = unsafe { libc::dup(proxy_fd) };
        Command::new("cloudflared")
            .args(["access", "ssh", "--hostname", "ssh.gojoe.dev"])
            .stdin(unsafe { Stdio::from_raw_fd(proxy_fd) })
            .stdout(unsafe { Stdio::from_raw_fd(stdout_fd) })
            .spawn()?;

        let mut session = Session::new()?;
        session.set_tcp_stream(our_socket);
        session.handshake()?;
        session.userauth_agent("deepwater")?;

        Ok(Self { session })
    }
    pub fn exec(&self, cmd: &str) -> Result<String, Box<dyn std::error::Error>> {
        println!("executing command: {}", cmd);
        let mut channel = self.session.channel_session()?;
        // this may not work but we still want to continue even if it fails
        // also apparently it could say it worked even if it didn't so
        // I'm not even bothering to check the response
        //
        // rust should be installed before using this tool which currently
        // just deploys rust programs
        let _ = channel.setenv(
            "PATH",
            "/home/deepwater/.cargo/bin:/usr/local/bin:/usr/bin:/bin",
        );
        channel.exec(&format!("bash -l -c '{}'", cmd))?;
        let mut output = String::new();
        let mut buffer = vec![0; 1024];
        while channel.read(&mut buffer)? > 0 {
            let chunk = String::from_utf8_lossy(&buffer);
            print!("{}", chunk);
            output.push_str(&chunk);
        }
        channel.wait_close()?;

        let exit_status = channel.exit_status()?;
        if exit_status != 0 {
            return Err(format!("command failed with exit status {}", exit_status).into());
        }

        Ok(output)
    }
    pub fn scp(
        &self,
        local_path: &str,
        remote_path: &str,
    ) -> Result<u64, Box<dyn std::error::Error>> {
        let mut local_file = File::open(local_path)?;
        let metadata = local_file.metadata()?;
        let mut remote_file =
            self.session
                .scp_send(Path::new(remote_path), 0o644, metadata.len(), None)?;
        let bytes_sent = std::io::copy(&mut local_file, &mut remote_file)?;
        Ok(bytes_sent)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn execute_command() -> Result<(), Box<dyn std::error::Error>> {
        let ssh = CloudflareSsh::new().expect("Failed to create ssh client");
        let output = ssh.exec("ls -la").expect("Failed to execute command");
        println!("{}", output);
        let output = ssh
            .exec("ls -la argo")
            .expect("Failed to execute second command");
        println!("{}", output);
        Ok(())
    }
    #[test]
    fn upload_file() -> Result<(), Box<dyn std::error::Error>> {
        let ssh = CloudflareSsh::new().expect("Failed to create ssh client");
        ssh.scp("/Users/deepwater/file.txt", "/home/deepwater/file.txt")
            .expect("Failed to scp file");
        Ok(())
    }
}
