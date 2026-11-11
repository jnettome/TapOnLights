//! Check whether TapOnLights can talk to your bulbs, and how fast.
//!
//! Logs in to each bulb with the `tapo` crate (the library the app uses), reads
//! its state a few times over the open session and reports timings. With
//! `--write`, it also nudges the brightness of bulbs that are on by 1% and puts
//! it back, to time real commands. Bulbs that are off are never turned on.
//!
//! With `--stress <seconds>`, it sends commands to all the given bulbs at once,
//! as fast as each accepts them (one request in flight per bulb, like the app),
//! and reports the rate, latency percentiles and errors: the budget for effects.
//! It alternates brightness by 1% (not visible) and restores it at the end.
//!
//! Usage:
//!   set TAPO_USERNAME=you@example.com
//!   tapo-probe 192.168.1.42 [192.168.1.43 ...] [--write] [--stress 20]
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
    let stress: Option<u64> = args
        .iter()
        .position(|a| a == "--stress")
        .map(|i| args.get(i + 1).and_then(|v| v.parse().ok()).unwrap_or(20));
    let ips: Vec<&String> = args
        .iter()
        .enumerate()
        .filter(|(i, a)| !a.starts_with("--") && !(*i > 0 && args[i - 1] == "--stress"))
        .map(|(_, a)| a)
        .collect();
    if ips.is_empty() {
        eprintln!(
            "usage: tapo-probe <bulb IP>... [--write] [--stress <seconds>]\n(set TAPO_USERNAME; TAPO_PASSWORD is optional)"
        );
        std::process::exit(2);
    }
    let username = std::env::var("TAPO_USERNAME")
        .map_err(|_| "set TAPO_USERNAME to your Tapo account email")?;
    let password = match std::env::var("TAPO_PASSWORD") {
        Ok(p) => p,
        Err(_) => rpassword::prompt_password("Tapo password: ")?,
    };

    if let Some(seconds) = stress {
        let mut tasks = tokio::task::JoinSet::new();
        for ip in &ips {
            let (ip, user, pass) = (ip.to_string(), username.clone(), password.clone());
            tasks.spawn(
                async move { stress_bulb(ip, user, pass, Duration::from_secs(seconds)).await },
            );
        }
        println!("stressing {} bulb(s) for {seconds} s…", ips.len());
        while let Some(report) = tasks.join_next().await {
            println!("{}", report?);
        }
        return Ok(());
    }

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

/// Send brightness changes back to back (one in flight) for `duration` and
/// report throughput, latency percentiles and failures.
async fn stress_bulb(ip: String, user: String, pass: String, duration: Duration) -> String {
    let login = ApiClient::new(&user, &pass)
        .with_timeout(Duration::from_secs(4))
        .l530(ip.as_str())
        .await;
    let mut handler = match login {
        Ok(h) => h,
        Err(e) => return format!("\n== {ip}\nlogin FAILED: {e}"),
    };
    let info = match handler.get_device_info().await {
        Ok(i) => i,
        Err(e) => return format!("\n== {ip}\nread FAILED: {e}"),
    };
    if !info.device_on {
        return format!("\n== {ip}\nbulb is off: stress test skipped so it isn't turned on");
    }
    let original = info.brightness;
    let nudged = if original > 1 { original - 1 } else { 2 };

    let mut latencies: Vec<u128> = Vec::new();
    let (mut errors, mut timeouts, mut relogins) = (0u32, 0u32, 0u32);
    let mut first_error = None;
    let started = Instant::now();
    let mut i = 0u64;
    while started.elapsed() < duration {
        let value = if i.is_multiple_of(2) {
            nudged
        } else {
            original
        };
        i += 1;
        let t = Instant::now();
        match tokio::time::timeout(Duration::from_secs(2), handler.set_brightness(value)).await {
            Ok(Ok(())) => latencies.push(t.elapsed().as_millis()),
            Ok(Err(e)) => {
                errors += 1;
                first_error.get_or_insert_with(|| e.to_string());
                if handler.refresh_session().await.is_ok() {
                    relogins += 1;
                }
            }
            Err(_) => {
                timeouts += 1;
                if handler.refresh_session().await.is_ok() {
                    relogins += 1;
                }
            }
        }
    }
    let elapsed = started.elapsed().as_secs_f64();
    let _ = handler.set_brightness(original).await;
    let restored = handler.get_device_info().await.map(|i| i.brightness).ok();

    latencies.sort_unstable();
    let pct = |p: f64| -> u128 {
        if latencies.is_empty() {
            return 0;
        }
        latencies[((latencies.len() - 1) as f64 * p).round() as usize]
    };
    let gaps_over_500 = latencies.iter().filter(|&&l| l > 500).count();
    format!(
        "\n== {ip}\n\
         ok {} in {elapsed:.1} s = {:.1} changes/s\n\
         latency ms: min {} · p50 {} · p90 {} · p99 {} · max {}\n\
         slow (>500 ms) {gaps_over_500} · errors {errors} · timeouts (>2 s) {timeouts} · re-logins {relogins}{}\n\
         brightness restored to {} (was {original})",
        latencies.len(),
        latencies.len() as f64 / elapsed,
        latencies.first().copied().unwrap_or(0),
        pct(0.5),
        pct(0.9),
        pct(0.99),
        latencies.last().copied().unwrap_or(0),
        first_error
            .map(|e| format!("\nfirst error: {e}"))
            .unwrap_or_default(),
        restored
            .map(|b| b.to_string())
            .unwrap_or_else(|| "?".into()),
    )
}
