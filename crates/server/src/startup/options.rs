use fagent_server::RenderBackend;
use std::{
    env,
    net::{IpAddr, Ipv4Addr},
};

pub(super) fn render_backend(cli: Option<RenderBackend>) -> Result<RenderBackend, &'static str> {
    const INVALID: &str = "RENDER_BACKEND must be chromium or vello";
    if let Some(backend) = cli {
        return Ok(backend);
    }
    match env::var("RENDER_BACKEND") {
        Ok(value) => value.parse().map_err(|_| INVALID),
        Err(env::VarError::NotPresent) => Ok(RenderBackend::BUILD_DEFAULT),
        Err(_) => Err(INVALID),
    }
}

pub(super) fn trusted_proxy() -> Result<Option<IpAddr>, &'static str> {
    const INVALID: &str = "FAGENT_PUBLIC_TRUSTED_PROXY_IP must be a single IP address";
    match env::var("FAGENT_PUBLIC_TRUSTED_PROXY_IP") {
        Ok(value) if value.is_empty() => Ok(None),
        Ok(value) => value
            .parse::<IpAddr>()
            .ok()
            .map(|ip| ip.to_canonical())
            .filter(|ip| !ip.is_unspecified())
            .map(Some)
            .ok_or(INVALID),
        Err(env::VarError::NotPresent) => Ok(None),
        Err(_) => Err(INVALID),
    }
}

const LIVE_UPDATES_ENV: &str = "FAGENT_PUBLIC_LIVE_UPDATES";
const LIVE_UPDATES_INVALID: &str = "FAGENT_PUBLIC_LIVE_UPDATES must be on or off";

/// `on`/`off` (also `1`/`0`, `true`/`false`) for a public viewer switch; anything else fails.
pub(super) fn switch(value: &str) -> Option<bool> {
    match value {
        "on" | "1" | "true" => Some(true),
        "off" | "0" | "false" => Some(false),
        _ => None,
    }
}

/// Public live updates default to off. The CLI flag overrides the environment; empty is unset.
pub(super) fn live_updates(cli: Option<bool>) -> Result<bool, &'static str> {
    if let Some(enabled) = cli {
        return Ok(enabled);
    }
    match env::var(LIVE_UPDATES_ENV) {
        Ok(value) if value.is_empty() => Ok(false),
        Ok(value) => switch(&value).ok_or(LIVE_UPDATES_INVALID),
        Err(env::VarError::NotPresent) => Ok(false),
        Err(_) => Err(LIVE_UPDATES_INVALID),
    }
}

const OVERSCAN_ENV: &str = "FAGENT_VIEWER_OVERSCAN";
const OVERSCAN_INVALID: &str =
    "FAGENT_VIEWER_OVERSCAN must be a percent per side from 0 to 50 (for example 10 or 12.5)";

/// Percent per side as plain digits with an optional fraction (`10`, `12.5`), 0 to 50; returns
/// the ratio (`0.125`). Signs, exponents, `inf`/`nan` and surrounding spaces are rejected.
fn overscan_ratio(value: &str) -> Option<f64> {
    let (whole, fraction) = value.split_once('.').unwrap_or((value, "0"));
    let digits = |part: &str| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit());
    if !digits(whole) || !digits(fraction) || value.len() > 16 {
        return None;
    }
    let percent: f64 = value.parse().ok()?;
    (0.0..=50.0).contains(&percent).then(|| percent / 100.0)
}

/// Viewer overscan (`/api/v1/viewer` `overscan`) defaults to 0, off. Empty is unset.
pub(super) fn viewer_overscan() -> Result<f64, &'static str> {
    match env::var(OVERSCAN_ENV) {
        Ok(value) if value.is_empty() => Ok(0.0),
        Ok(value) => overscan_ratio(&value).ok_or(OVERSCAN_INVALID),
        Err(env::VarError::NotPresent) => Ok(0.0),
        Err(_) => Err(OVERSCAN_INVALID),
    }
}

pub(super) fn env_port() -> Result<u16, &'static str> {
    const INVALID: &str = "PORT must be a number from 0 to 65535";
    match env::var("PORT") {
        Ok(value) => value.parse().map_err(|_| INVALID),
        Err(env::VarError::NotPresent) => Ok(3847),
        Err(_) => Err(INVALID),
    }
}

/// Worker callbacks require IPv4 loopback to be included in the bind address.
pub(super) fn bind_host(value: &str) -> Result<Ipv4Addr, &'static str> {
    match value {
        "127.0.0.1" => Ok(Ipv4Addr::LOCALHOST),
        "0.0.0.0" => Ok(Ipv4Addr::UNSPECIFIED),
        _ => Err("--host must be 127.0.0.1 or 0.0.0.0"),
    }
}

#[cfg(test)]
mod tests {
    use super::overscan_ratio;

    #[test]
    fn overscan_accepts_percent_from_0_to_50() {
        for (value, ratio) in [
            ("0", 0.0),
            ("0.0", 0.0),
            ("10", 0.1),
            ("12.5", 0.125),
            ("050", 0.5),
            ("50", 0.5),
            ("50.000", 0.5),
        ] {
            assert_eq!(overscan_ratio(value), Some(ratio), "{value}");
        }
    }

    #[test]
    fn overscan_rejects_anything_else() {
        for value in [
            "50.01", "51", "100", "-1", "-0", "+10", "1e1", "inf", "NaN", "10.", ".5", "1.2.3",
            " 10", "10 ", "10%", "0.1x", "ten", "0x10",
        ] {
            assert_eq!(overscan_ratio(value), None, "{value}");
        }
    }
}
