use std::fs::File;
use std::io::Read;
use std::os::fd::FromRawFd;
use std::os::unix::io::IntoRawFd;
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::process::Command;
use std::process::Stdio;

use ssh2::Session;
struct CloudflareSsh {
    session: Session,
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

        let mut session = Session::new().expect("Failed to create session");
        session.set_tcp_stream(our_socket);
        session.handshake()?;
        session.userauth_agent("deepwater")?;

        Ok(Self { session })
    }
    pub fn exec(&self, cmd: &str) -> Result<String, Box<dyn std::error::Error>> {
        let mut channel = self.session.channel_session()?;
        channel.exec(cmd)?;
        let mut output = String::new();
        channel.read_to_string(&mut output)?;
        Ok(output)
    }
    pub fn scp(
        &self,
        local_path: &str,
        remote_path: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let local_file = File::open(local_path).expect("Failed to open local file");
        let metadata = local_file.metadata()?;
        let mut remote_file =
            self.session
                .scp_send(Path::new(remote_path), 0o644, metadata.len(), None)?;
        std::io::copy(&mut File::open(local_path)?, &mut remote_file)?;
        Ok(())
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
