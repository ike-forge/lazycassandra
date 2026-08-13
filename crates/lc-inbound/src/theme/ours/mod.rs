//! The palettes written here, and why each declares its own background.
//!
//! # Contrast is a relationship, not a property
//!
//! No colour is readable on its own — only against a specific background.
//! `#8a4fd4`, the `keyspace` of the palette this replaces, is 5.1:1 on white
//! and 2.4:1 on `#303446`. Same hex, both true. If the background is unknown
//! at author time, the legibility of every foreground is also unknown, and no
//! care in picking hexes can fix that.
//!
//! # Hedging across unknown backgrounds fails on all of them
//!
//! Faced with an unknown ground the only available move is a mid-lightness
//! colour that works on both. That band — OKLCH L* 0.55 to 0.67 — is exactly
//! the band too dark to read on a dark terminal and too light on a light one.
//! 12 of the old palette's 13 roles fell below 4.5:1 on dark terminals *and*
//! 12 of 13 fell below it on light ones.
//!
//! # An unknown background makes the invariant untestable
//!
//! This is the practical argument. With a declared `base`, "no text colour
//! drops below 4.5:1" is a build-failing test. With an inherited one the same
//! claim is unfalsifiable, so contrast regressions ship silently and are found
//! only by a user filing "the purple is invisible".
//!
//! A full-screen TUI is not sharing the surface either: `LazyCassandra` takes
//! the alternate screen buffer and paints every cell, so inheriting the
//! terminal background blends it into nothing — there is no surrounding
//! session left on screen. Users who run a translucent or image background
//! have a real preference, and it lands as an explicit `background =
//! "terminal"` opt-in with the config file (vision.md §6), never as the
//! default.
//!
//! Design rules, in full, live in
//! docs/superpowers/specs/2026-08-13-colour-palettes-design.md §3.1.

pub(super) mod ash;
pub(super) mod beacon;
pub(super) mod default;
pub(super) mod quill;
