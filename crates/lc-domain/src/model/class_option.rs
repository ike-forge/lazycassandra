//! `compaction` and `compression` read as the class they name and its settings.
//! Refuses to guess: anything unknown yields `None` and the raw text is drawn
//! instead.

/// Keeps a compressor's keys out of a `compaction` value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClassKind {
    Compaction,
    Compression,
}

impl ClassKind {
    #[must_use]
    pub fn of(option: &str) -> Option<Self> {
        match option {
            "compaction" => Some(Self::Compaction),
            "compression" => Some(Self::Compression),
            _ => None,
        }
    }

    const fn package(self) -> &'static str {
        match self {
            Self::Compaction => "org.apache.cassandra.db.compaction.",
            Self::Compression => "org.apache.cassandra.io.compress.",
        }
    }

    const fn classes(self) -> &'static [Class] {
        match self {
            Self::Compaction => COMPACTION,
            Self::Compression => COMPRESSION,
        }
    }

    /// Drawn after the class's own keys.
    const fn shared(self) -> &'static [Key] {
        match self {
            Self::Compaction => COMMON,
            Self::Compression => CHUNKING,
        }
    }
}

/// Cassandra's name, the drawn label, and the unit the name states and the
/// label drops.
type Key = (&'static str, &'static str, &'static str);

const SEPARATOR: &str = ", ";

/// In the class's declared order, not the map's alphabetical one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClassOption {
    /// The package is the same for every class of a kind.
    pub class: String,
    pub settings: Vec<Setting>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Setting {
    pub label: &'static str,
    /// The key's unit is appended where the label dropped it.
    pub value: String,
}

struct Class {
    name: &'static str,
    /// Class-specific keys first, next to the name that gives them meaning.
    keys: &'static [Key],
}

const COMMON: &[Key] = &[
    ("tombstone_threshold", "tombstone", ""),
    ("tombstone_compaction_interval", "interval", "s"),
    ("unchecked_tombstone_compaction", "unchecked", ""),
    ("only_purge_repaired_tombstones", "repaired only", ""),
    ("provide_overlapping_tombstones", "overlapping", ""),
    ("log_all", "log all", ""),
    ("enabled", "enabled", ""),
];

const COMPACTION: &[Class] = &[
    Class {
        name: "SizeTieredCompactionStrategy",
        keys: &[
            ("max_threshold", "max", ""),
            ("min_threshold", "min", ""),
            ("bucket_low", "bucket low", ""),
            ("bucket_high", "bucket high", ""),
            ("min_sstable_size", "min sstable", ""),
        ],
    },
    Class {
        name: "LeveledCompactionStrategy",
        keys: &[
            ("sstable_size_in_mb", "sstable", "mb"),
            ("fanout_size", "fanout", ""),
            ("single_sstable_uplevel", "uplevel", ""),
        ],
    },
    Class {
        name: "TimeWindowCompactionStrategy",
        keys: &[
            ("compaction_window_size", "window", ""),
            ("compaction_window_unit", "unit", ""),
            ("timestamp_resolution", "resolution", ""),
            (
                "expired_sstable_check_frequency_seconds",
                "expiry check",
                "s",
            ),
            (
                "unsafe_aggressive_sstable_expiration",
                "aggressive expiry",
                "",
            ),
            ("max_threshold", "max", ""),
            ("min_threshold", "min", ""),
        ],
    },
    Class {
        name: "UnifiedCompactionStrategy",
        keys: &[
            ("scaling_parameters", "scaling", ""),
            ("target_sstable_size", "target", ""),
            ("min_sstable_size", "min sstable", ""),
            ("base_shard_count", "shards", ""),
            ("sstable_growth", "growth", ""),
            ("max_sstables_to_compact", "max sstables", ""),
            ("overlap_inclusion_method", "overlap", ""),
            (
                "expired_sstable_check_frequency_seconds",
                "expiry check",
                "s",
            ),
            (
                "unsafe_aggressive_sstable_expiration",
                "aggressive expiry",
                "",
            ),
        ],
    },
    // Removed in 4.0; here only for 3.11 clusters.
    Class {
        name: "DateTieredCompactionStrategy",
        keys: &[
            ("max_sstable_age_days", "max age", "d"),
            ("base_time_seconds", "base", "s"),
            ("max_window_size_seconds", "max window", "s"),
            ("timestamp_resolution", "resolution", ""),
            ("max_threshold", "max", ""),
            ("min_threshold", "min", ""),
        ],
    },
];

const CHUNKING: &[Key] = &[
    ("chunk_length_in_kb", "chunk", "kb"),
    ("crc_check_chance", "crc", ""),
    ("enabled", "enabled", ""),
];

const COMPRESSION: &[Class] = &[
    Class {
        name: "LZ4Compressor",
        keys: &[
            ("lz4_compressor_type", "type", ""),
            ("lz4_high_compressor_level", "level", ""),
        ],
    },
    Class {
        name: "ZstdCompressor",
        keys: &[("compression_level", "level", "")],
    },
    Class {
        name: "SnappyCompressor",
        keys: &[],
    },
    Class {
        name: "DeflateCompressor",
        keys: &[],
    },
    Class {
        name: "NoopCompressor",
        keys: &[],
    },
];

impl ClassOption {
    /// A known class with any unknown key is `None`: a summary that drops a
    /// setting is worse than the raw map.
    #[must_use]
    pub fn parse(kind: ClassKind, text: &str) -> Option<Self> {
        let entries = entries(text)?;
        let class = value_of(&entries, "class")?;
        let class = shortened(kind, class)?;
        let known = kind.classes().iter().find(|known| known.name == class)?;
        let keys: Vec<&Key> = known.keys.iter().chain(kind.shared()).collect();

        let recognised = |key: &str| key == "class" || keys.iter().any(|(name, ..)| *name == key);
        if !entries.iter().all(|(key, _)| recognised(key)) {
            return None;
        }

        let settings = keys
            .into_iter()
            .filter_map(|(name, label, unit)| {
                value_of(&entries, name).map(|value| Setting {
                    label,
                    value: format!("{value}{unit}"),
                })
            })
            .collect();

        Some(Self {
            class: class.to_owned(),
            settings,
        })
    }
}

/// A value containing a separator is indistinguishable from another entry
/// (`scaling_parameters` is `T4, T4, N`), so it yields `None`.
fn entries(text: &str) -> Option<Vec<(&str, &str)>> {
    let inner = text.strip_prefix('{')?.strip_suffix('}')?;
    if inner.is_empty() {
        return None;
    }

    let mut entries: Vec<(&str, &str)> = Vec::new();

    for entry in inner.split(SEPARATOR) {
        let (key, value) = entry.split_once(": ")?;
        if key.is_empty() || value.is_empty() || value_of(&entries, key).is_some() {
            return None;
        }
        entries.push((key, value));
    }

    Some(entries)
}

fn value_of<'a>(entries: &[(&'a str, &'a str)], key: &str) -> Option<&'a str> {
    entries
        .iter()
        .find(|(name, _)| *name == key)
        .map(|(_, value)| *value)
}

/// Any other package is not a class this build knows, whatever its short name.
fn shortened(kind: ClassKind, class: &str) -> Option<&str> {
    let short = class.strip_prefix(kind.package()).unwrap_or(class);

    (!short.contains('.')).then_some(short)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings(option: &ClassOption) -> Vec<(&str, &str)> {
        option
            .settings
            .iter()
            .map(|setting| (setting.label, setting.value.as_str()))
            .collect()
    }

    #[test]
    fn a_strategy_keeps_its_own_keys_in_the_order_the_class_declares_them() {
        let parsed = ClassOption::parse(
            ClassKind::Compaction,
            "{class: org.apache.cassandra.db.compaction.SizeTieredCompactionStrategy, \
             max_threshold: 32, min_threshold: 4}",
        )
        .expect("every key here is one STCS accepts");

        assert_eq!(parsed.class, "SizeTieredCompactionStrategy");
        assert_eq!(settings(&parsed), [("max", "32"), ("min", "4")]);
    }

    #[test]
    fn a_key_naming_its_unit_moves_that_unit_onto_the_value() {
        let parsed = ClassOption::parse(
            ClassKind::Compression,
            "{chunk_length_in_kb: 16, class: org.apache.cassandra.io.compress.LZ4Compressor}",
        )
        .expect("chunk length is the option every compressor takes");

        assert_eq!(parsed.class, "LZ4Compressor");
        assert_eq!(settings(&parsed), [("chunk", "16kb")]);
    }

    #[test]
    fn the_keys_every_strategy_shares_follow_the_ones_only_this_one_has() {
        let parsed = ClassOption::parse(
            ClassKind::Compaction,
            "{class: org.apache.cassandra.db.compaction.LeveledCompactionStrategy, \
             enabled: false, sstable_size_in_mb: 160, tombstone_threshold: 0.2}",
        )
        .expect("the common keys belong to every strategy");

        assert_eq!(
            settings(&parsed),
            [
                ("sstable", "160mb"),
                ("tombstone", "0.2"),
                ("enabled", "false"),
            ]
        );
    }

    #[test]
    fn a_class_named_without_its_package_is_the_same_class() {
        let parsed = ClassOption::parse(
            ClassKind::Compaction,
            "{class: TimeWindowCompactionStrategy, compaction_window_size: 1, \
             compaction_window_unit: DAYS}",
        )
        .expect("CQL accepts the bare name, so a cluster may report it");

        assert_eq!(parsed.class, "TimeWindowCompactionStrategy");
        assert_eq!(settings(&parsed), [("window", "1"), ("unit", "DAYS")]);
    }

    #[test]
    fn a_key_this_build_does_not_know_falls_back_rather_than_dropping_it() {
        assert_eq!(
            ClassOption::parse(
                ClassKind::Compaction,
                "{class: org.apache.cassandra.db.compaction.SizeTieredCompactionStrategy, \
                 max_threshold: 32, some_future_knob: 7}",
            ),
            None,
            "a summary that hides a setting is worse than the map it replaced"
        );
    }

    #[test]
    fn a_class_this_build_does_not_know_falls_back() {
        assert_eq!(
            ClassOption::parse(
                ClassKind::Compaction,
                "{class: com.example.MyCompactionStrategy, min_threshold: 4}",
            ),
            None
        );
    }

    #[test]
    fn a_class_from_the_other_kinds_package_falls_back() {
        assert_eq!(
            ClassOption::parse(
                ClassKind::Compression,
                "{class: org.apache.cassandra.db.compaction.SizeTieredCompactionStrategy}",
            ),
            None
        );

        assert_eq!(
            ClassOption::parse(
                ClassKind::Compaction,
                "{class: org.apache.cassandra.io.compress.LZ4Compressor}",
            ),
            None,
            "the package has to match the kind, not merely be a package"
        );
    }

    #[test]
    fn a_class_key_naming_a_known_class_under_the_wrong_package_falls_back() {
        assert_eq!(
            ClassOption::parse(
                ClassKind::Compaction,
                "{class: com.example.SizeTieredCompactionStrategy, min_threshold: 4}",
            ),
            None
        );
    }

    #[test]
    fn a_value_holding_the_entry_separator_falls_back() {
        assert_eq!(
            ClassOption::parse(
                ClassKind::Compaction,
                "{class: org.apache.cassandra.db.compaction.UnifiedCompactionStrategy, \
                 scaling_parameters: T4, T4, N}",
            ),
            None,
            "a flattened map cannot tell this value's commas from its own"
        );
    }

    #[test]
    fn a_map_with_no_class_falls_back() {
        assert_eq!(
            ClassOption::parse(ClassKind::Compression, "{enabled: false}"),
            None,
            "disabled compression names no compressor, so there is nothing to summarise"
        );
    }

    #[test]
    fn text_that_is_not_a_map_falls_back() {
        for text in [
            "",
            "''",
            "{",
            "{}",
            "{class}",
            "{class: LZ4Compressor",
            "class: LZ4Compressor}",
        ] {
            assert_eq!(
                ClassOption::parse(ClassKind::Compression, text),
                None,
                "{text} is not a flattened map"
            );
        }
    }

    #[test]
    fn a_key_repeated_falls_back() {
        assert_eq!(
            ClassOption::parse(
                ClassKind::Compaction,
                "{class: SizeTieredCompactionStrategy, min_threshold: 4, min_threshold: 8}",
            ),
            None,
            "a map cannot hold a key twice, so this is not one"
        );
    }

    #[test]
    fn a_class_alone_parses_to_a_class_and_nothing_else() {
        let parsed = ClassOption::parse(ClassKind::Compression, "{class: SnappyCompressor}")
            .expect("a compressor with no settings is still a compressor");

        assert_eq!(parsed.class, "SnappyCompressor");
        assert!(parsed.settings.is_empty());
    }

    #[test]
    fn every_option_name_that_carries_a_class_is_recognised_by_that_name() {
        assert_eq!(ClassKind::of("compaction"), Some(ClassKind::Compaction));
        assert_eq!(ClassKind::of("compression"), Some(ClassKind::Compression));
        assert_eq!(ClassKind::of("comment"), None);
    }
}
