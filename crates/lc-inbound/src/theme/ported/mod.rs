//! Palettes taken from elsewhere, as their authors wrote them.
//!
//! Nothing here is contrast-gated. We assume the choices in a palette this
//! well-travelled are good enough, and the only judgement we exercise is
//! whether to ship it — which is why the sweep in `theme::tests` is written
//! against `Ours::ALL` and cannot name a [`Ported`](lc_domain::Ported) theme
//! at all (design §2).

pub(super) mod catppuccin;
