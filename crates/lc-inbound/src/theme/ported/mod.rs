//! Palettes taken from elsewhere, as their authors wrote them.
//!
//! Nothing here is held to our contrast rules. We assume the choices in a
//! palette this well-travelled are good enough, and the only judgement we
//! exercise is whether to ship it — which is why the sweep in `theme::tests`
//! is written against `Ours::ALL` and cannot name a
//! [`Ported`](lc_domain::Ported) theme at all (design §2).
//!
//! What each module here does check is its own transcription: a floor far
//! below 4.5:1, set where upstream already sits, catches a mistyped hex
//! without testing our taste against the authors'.

pub(super) mod catppuccin;
