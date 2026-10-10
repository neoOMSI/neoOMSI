use anyhow::{Context, Result, anyhow, bail};
use launcher_protocol::api::{Frame, Handshake, Request, StatusCode, frame, request, response};
use launcher_protocol::{encode, read_frame};
use std::io::Write;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

struct ChildGuard(Option<Child>);

impl ChildGuard {
    fn new(child: Child) -> Self {
        Self(Some(child))
    }

    fn as_mut(&mut self) -> &mut Child {
        self.0.as_mut().expect("child is not present")
    }

    fn wait_timeout(&mut self, timeout: Duration) -> Result<std::process::ExitStatus> {
        let start = std::time::Instant::now();
        let child = self.0.as_mut().expect("child is not present");
        loop {
            if let Some(status) = child.try_wait().context("failed checking child exit status")? {
                return Ok(status);
            }
            if start.elapsed() >= timeout {
                bail!("timed out after {timeout:?} waiting for child exit");
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

impl Drop for ChildGuard {
    fn drop(&mut self) {
        if let Some(mut child) = self.0.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

fn read_frame_timeout(
    rx_read: &mpsc::Receiver<Result<Option<Frame>>>,
    timeout: Duration,
) -> Result<Frame> {
    match rx_read.recv_timeout(timeout) {
        Ok(Ok(Some(frame))) => Ok(frame),
        Ok(Ok(None)) => bail!("unexpected EOF from engine standard output"),
        Ok(Err(e)) => Err(e),
        Err(mpsc::RecvTimeoutError::Timeout) => {
            bail!("timed out after {timeout:?} waiting for engine frame");
        }
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            bail!("stdout reader thread disconnected unexpectedly");
        }
    }
}

fn main() -> Result<()> {
    let exe = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "dist/windows/neoomsi.exe".into());
    println!("Testing --control-protocol handshake against: {exe}");

    let child = Command::new(&exe)
        .arg("--control-protocol")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .with_context(|| format!("failed to spawn {exe} --control-protocol"))?;

    let mut guard = ChildGuard::new(child);

    let mut stdin = guard
        .as_mut()
        .stdin
        .take()
        .ok_or_else(|| anyhow!("no stdin available on child process"))?;
    let mut stdout = guard
        .as_mut()
        .stdout
        .take()
        .ok_or_else(|| anyhow!("no stdout available on child process"))?;

    let (frame_tx, frame_rx) = mpsc::channel();
    std::thread::spawn(move || loop {
        match read_frame(&mut stdout) {
            Ok(Some(frame)) => {
                if frame_tx.send(Ok(Some(frame))).is_err() {
                    break;
                }
            }
            Ok(None) => {
                let _ = frame_tx.send(Ok(None));
                break;
            }
            Err(err) => {
                let _ = frame_tx.send(Err(err.into()));
                break;
            }
        }
    });

    let timeout = Duration::from_secs(10);

    // 1. Valid handshake
    println!("Sending valid handshake (version 1)...");
    let req1 = Frame {
        request_id: "req_valid".into(),
        body: Some(frame::Body::Request(Request {
            command: Some(request::Command::Handshake(Handshake {
                protocol_version: "1".into(),
                launcher_version: "0.3.0".into(),
                client_platform: std::env::consts::OS.into(),
            })),
        })),
        ..Default::default()
    };
    stdin.write_all(&encode(&req1)?)?;
    stdin.flush()?;

    let resp1 = read_frame_timeout(&frame_rx, timeout)?;
    if resp1.request_id != "req_valid" {
        bail!("expected request_id 'req_valid', got '{}'", resp1.request_id);
    }

    let hs_response = match resp1.body {
        Some(frame::Body::Response(r)) => match r.answer {
            Some(response::Answer::Handshake(h)) => h,
            other => bail!("unexpected answer variant: {other:?}"),
        },
        other => bail!("unexpected frame body variant: {other:?}"),
    };

    let status = hs_response
        .status
        .ok_or_else(|| anyhow!("missing status in handshake response"))?;
    if status.code != StatusCode::Ok as i32 {
        bail!("handshake status code was not OK (0): {}", status.code);
    }
    if hs_response.protocol_version != "1" {
        bail!(
            "expected protocol_version '1', got '{}'",
            hs_response.protocol_version
        );
    }
    if hs_response.engine_version.is_empty() {
        bail!("engine_version was empty in handshake response");
    }
    println!(
        "Valid handshake OK (protocol: {}, engine: {}, capabilities: {:?})",
        hs_response.protocol_version, hs_response.engine_version, hs_response.supported_capabilities
    );

    // 2. Incompatible version handshake
    println!("Sending incompatible handshake (version 999.0)...");
    let req2 = Frame {
        request_id: "req_invalid_ver".into(),
        body: Some(frame::Body::Request(Request {
            command: Some(request::Command::Handshake(Handshake {
                protocol_version: "999.0".into(),
                launcher_version: "0.3.0".into(),
                client_platform: std::env::consts::OS.into(),
            })),
        })),
        ..Default::default()
    };
    stdin.write_all(&encode(&req2)?)?;
    stdin.flush()?;

    let resp2 = read_frame_timeout(&frame_rx, timeout)?;
    if resp2.request_id != "req_invalid_ver" {
        bail!("expected request_id 'req_invalid_ver', got '{}'", resp2.request_id);
    }

    let hs_err = match resp2.body {
        Some(frame::Body::Response(r)) => match r.answer {
            Some(response::Answer::Handshake(h)) => h,
            other => bail!("unexpected answer variant: {other:?}"),
        },
        other => bail!("unexpected frame body variant: {other:?}"),
    };

    let err_status = hs_err
        .status
        .ok_or_else(|| anyhow!("missing status in invalid version response"))?;
    if err_status.code != StatusCode::UnsupportedVersion as i32 {
        bail!(
            "expected StatusCode::UnsupportedVersion (1), got {}",
            err_status.code
        );
    }
    println!(
        "Incompatible version rejected as expected: '{}' (code {})",
        err_status.message, err_status.code
    );

    // 3. Graceful shutdown
    println!("Sending shutdown command...");
    let req3 = Frame {
        request_id: "req_shutdown".into(),
        body: Some(frame::Body::Request(Request {
            command: Some(request::Command::Shutdown(launcher_protocol::api::Empty {})),
        })),
        ..Default::default()
    };
    stdin.write_all(&encode(&req3)?)?;
    stdin.flush()?;

    let resp3 = read_frame_timeout(&frame_rx, timeout)?;
    if resp3.request_id != "req_shutdown" {
        bail!("expected request_id 'req_shutdown', got '{}'", resp3.request_id);
    }

    let exit_status = guard.wait_timeout(timeout)?;
    if !exit_status.success() {
        bail!("engine exited with non-zero exit status: {exit_status}");
    }
    println!("Engine process terminated cleanly following shutdown command.");

    println!("All control protocol checks passed successfully!");
    Ok(())
}
