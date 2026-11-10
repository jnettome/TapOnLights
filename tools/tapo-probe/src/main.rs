//! Check whether TapOnLights can talk to your bulbs, and how fast.
//!
//! Logs in to each bulb with the `tapo` crate (the library the app uses), reads
//! its state a few times over the open session and reports timings. With
//! `--write`, it also nudges the brightness of bulbs that are on by 1% and puts
//! it back, to time real commands. Bulbs that are off are never turned on.
//!
//! Usage:
//!   set TAPO_USERNAME=you@example.com
//!   tapo-probe 192.168.1.42 [192.168.1.43 ...] [--write]
//!
//! The password comes from TAPO_PASSWORD or is asked for without echo.

use std::time::{Duration, Instant};

use tapo::ApiClient;

type Res<T> = Result<T, Box<dyn std::error::Error>>;

fn ms(t: Instant) -> String {
    format!("{:>5} ms", t.elapsed().as_millis())
}

#[tokio::main]
async fn main() -> Res<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let write = args.iter().any(|a| a == "--write");
    let ips: Vec<&String> = args.iter().filter(|a| !a.starts_with("--")).collect();
    if ips.is_empty() {
        eprintln!(
            "usage: tapo-probe <bulb IP>... [--write]\n(set TAPO_USERNAME; TAPO_PASSWORD is optional)"
        );
        std::process::exit(2);
    }
    let username = std::env::var("TAPO_USERNAME")
        .map_err(|_| "set TAPO_USERNAME to your Tapo account email")?;
    let password = match std::env::var("TAPO_PASSWORD") {
        Ok(p) => p,
        Err(_) => rpassword::prompt_password("Tapo password: ")?,
    };

    for ip in ips {
        println!("\n== {ip}");
        let t = Instant::now();
        let mut handler = match ApiClient::new(&username, &password)
            .with_timeout(Duration::from_secs(6))
            .l530(ip.as_str())
            .await
        {
            Ok(h) => {
                println!("login                     {}", ms(t));
                h
            }
            Err(e) => {
                println!("login FAILED after {}: {e}", ms(t));
                continue;
            }
        };

        let mut info = None;
        for _ in 0..3 {
            let t = Instant::now();
            let i = handler.get_device_info().await?;
            println!("read state (open session) {}", ms(t));
            info = Some(i);
        }
        let info = info.expect("read three times");
        println!(
            "   model={} hw={} fw={} on={} brightness={} hue={:?} saturation={:?} color_temp={} signal={}",
            info.model,
            info.hw_ver,
            info.fw_ver,
            info.device_on,
            info.brightness,
            info.hue,
            info.saturation,
            info.color_temp,
            info.signal_level
        );

        if write {
            if info.device_on {
                let original = info.brightness;
                let nudged = if original > 1 { original - 1 } else { 2 };
                for b in [nudged, original, nudged, original] {
                    let t = Instant::now();
                    handler.set_brightness(b).await?;
                    println!("set brightness {b:>3}%       {}", ms(t));
                }
                let back = handler.get_device_info().await?;
                println!(
                    "   brightness after test: {} (was {original})",
                    back.brightness
                );
            } else {
                println!("   bulb is off: write test skipped so it isn't turned on");
            }
        }

        let t = Instant::now();
        handler.refresh_session().await?;
        println!("new session               {}", ms(t));
    }
    Ok(())
}
