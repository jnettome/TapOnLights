//! Probe: can the Rust `tapo` crate log in to the configured bulbs, read state,
//! and send writes over a kept-open session, and how long does each step take?
//!
//! Reads bulbs and the account email from the Python app's settings.json and the
//! password from Windows Credential Manager (where python-keyring stored it).
//! Usage: tapo-probe [IP-to-run-write-test-on]
//! The write test nudges brightness by 1% and restores it; it is skipped if the
//! bulb is off, so the probe never turns a bulb on.

use std::time::{Duration, Instant};

use tapo::ApiClient;

type Res<T> = Result<T, Box<dyn std::error::Error>>;

fn ms(t: Instant) -> String {
    format!("{:>5} ms", t.elapsed().as_millis())
}

fn password_for(username: &str) -> Res<String> {
    // python-keyring's WinVaultKeyring uses target "<service>", or "<user>@<service>"
    // when a second account was saved under the same service.
    let service = "Tapo Desktop";
    for target in [service.to_string(), format!("{username}@{service}")] {
        if let Ok(pw) = keyring::Entry::new_with_target(&target, service, username)?.get_password() {
            return Ok(pw);
        }
    }
    Err("no password for this account in Windows Credential Manager".into())
}

#[tokio::main]
async fn main() -> Res<()> {
    let appdata = std::env::var("APPDATA")?;
    let raw = std::fs::read_to_string(format!("{appdata}\\Tapo Desktop\\settings.json"))?;
    let settings: serde_json::Value = serde_json::from_str(&raw)?;
    let username = settings["username"].as_str().ok_or("no username in settings")?.to_string();
    let password = password_for(&username)?;
    let write_ip = std::env::args().nth(1);

    for dev in settings["devices"].as_array().ok_or("no devices")? {
        let ip = dev["ip"].as_str().ok_or("device without ip")?;
        println!("\n== {ip} ({})", dev["name"].as_str().unwrap_or(""));

        let t = Instant::now();
        let mut handler = match ApiClient::new(&username, &password)
            .with_timeout(Duration::from_secs(6))
            .l530(ip)
            .await
        {
            Ok(h) => {
                println!("login (protocol detect + handshake) {}", ms(t));
                h
            }
            Err(e) => {
                println!("login FAILED after {}: {e:?}", ms(t));
                continue;
            }
        };

        let mut info = None;
        for _ in 0..3 {
            let t = Instant::now();
            let i = handler.get_device_info().await?;
            println!("get_device_info (warm session)      {}", ms(t));
            info = Some(i);
        }
        let info = info.unwrap();
        println!(
            "   model={} hw={} fw={} on={} brightness={} hue={:?} sat={:?} temp={} rssi={}",
            info.model, info.hw_ver, info.fw_ver, info.device_on, info.brightness,
            info.hue, info.saturation, info.color_temp, info.rssi
        );

        if write_ip.as_deref() == Some(ip) {
            if info.device_on {
                let orig = info.brightness;
                let alt = if orig > 1 { orig - 1 } else { 2 };
                for b in [alt, orig, alt, orig] {
                    let t = Instant::now();
                    handler.set_brightness(b).await?;
                    println!("set_brightness({b:>3})                {}", ms(t));
                }
                let t = Instant::now();
                let back = handler.get_device_info().await?;
                println!("verify read                         {}  brightness={}", ms(t), back.brightness);
            } else {
                println!("   bulb is off: write test skipped so the probe does not turn it on");
            }
        }

        let t = Instant::now();
        handler.refresh_session().await?;
        println!("refresh_session (re-handshake)      {}", ms(t));
        let t = Instant::now();
        handler.get_device_info().await?;
        println!("get_device_info after refresh       {}", ms(t));
    }
    Ok(())
}
