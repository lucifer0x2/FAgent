/// Selects the compute and PNG pipeline. The default is fixed at build time by
/// `FAGENT_RENDER_BACKEND` (see [`RenderBackend::BUILD_DEFAULT`]); `RENDER_BACKEND` and
/// `--render-backend` still choose at startup.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RenderBackend {
    /// Node worker: `layout-step` + Chromium text measurement, Canvas2D PNG in Chromium.
    Chromium,
    /// Native compute, `vello_cpu` PNG (`fagent-paint-vello`), the viewer's encoder.
    Vello,
}

/// Build-time selection: unset or empty is `chromium`; any other value fails compilation.
const fn build_default(value: Option<&str>) -> RenderBackend {
    let Some(value) = value else {
        return RenderBackend::Chromium;
    };
    match value.as_bytes() {
        b"" | b"chromium" => RenderBackend::Chromium,
        b"vello" => RenderBackend::Vello,
        _ => panic!("FAGENT_RENDER_BACKEND must be chromium or vello"),
    }
}

impl RenderBackend {
    /// The backend this binary uses when neither `RENDER_BACKEND` nor `--render-backend` is
    /// given: `FAGENT_RENDER_BACKEND` at compile time, `chromium` when it was unset.
    pub const BUILD_DEFAULT: Self = build_default(option_env!("FAGENT_RENDER_BACKEND"));

    /// In-process compute and paint: no Node worker, Playwright or `layout-step`.
    pub fn is_native(self) -> bool {
        self == Self::Vello
    }

    /// The `RENDER_BACKEND` / `--render-backend` spelling.
    pub fn name(self) -> &'static str {
        match self {
            Self::Chromium => "chromium",
            Self::Vello => "vello",
        }
    }
}

impl Default for RenderBackend {
    fn default() -> Self {
        Self::BUILD_DEFAULT
    }
}

impl std::str::FromStr for RenderBackend {
    type Err = &'static str;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "chromium" => Ok(Self::Chromium),
            "vello" => Ok(Self::Vello),
            _ => Err("Render backend must be chromium or vello"),
        }
    }
}
