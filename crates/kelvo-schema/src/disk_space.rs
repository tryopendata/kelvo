//! Local, on-demand disk space reports (plan/disk-space-report.md). Session data held in
//! memory while the dashboard is open: not metric series, not stored, not on the wire.
//! Byte counts are `f64` so they cross to TS as `number`; they stay far below 2^53.
use serde::{Deserialize, Serialize};

/// A volume the user can scan, as Finder shows it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, specta::Type)]
pub struct SpaceVolume {
    /// The browsable mount point ("/" for the boot volume). Used as the volume's id.
    pub mount: String,
    /// Finder's name ("Macintosh HD").
    pub name: String,
    pub is_boot: bool,
    /// Used space Finder reports for the volume (its APFS container for APFS volumes).
    pub used_bytes: f64,
    pub total_bytes: f64,
    /// Locations macOS protects with privacy prompts. Skipped unless included by key.
    pub protected: Vec<SpaceProtected>,
}

/// A privacy-protected location (TCC) that a default scan does not enter.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct SpaceProtected {
    /// Path relative to the scan root; passed back in `include`.
    pub key: String,
    /// Display name ("Downloads", "App containers").
    pub label: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum SpacePhase {
    Idle,
    Scanning,
    Done,
    Cancelled,
    Failed,
}

/// Progress of the current (or last) scan. Polled while a scan runs.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, specta::Type)]
pub struct SpaceScanStatus {
    pub phase: SpacePhase,
    /// Report id of the scan this describes; queries use it.
    pub report_id: Option<u32>,
    pub volume: Option<String>,
    pub elapsed_ms: f64,
    /// Files counted so far.
    pub items: f64,
    /// Bytes measured in files so far.
    pub measured_bytes: f64,
    /// The volume's used bytes, the denominator for progress ("about 62%").
    pub used_bytes: f64,
    /// The folder being read, relative to the volume root. Absent when not scanning.
    pub current: Option<String>,
    pub error: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum SpaceKind {
    Folder,
    File,
    /// Children too small to show, merged into one entry.
    Smaller,
    /// Another volume in the same APFS container (System, VM, Preboot...).
    Volume,
    /// Used space no measured file or volume accounts for: snapshots, APFS metadata,
    /// unreadable and skipped folders, unmounted volumes. Not measured, so drawn as a gap.
    Other,
}

/// One entry in a folder's breakdown.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, specta::Type)]
pub struct SpaceNode {
    /// Stable within one report. Synthetic entries (smaller, volumes, other) have none.
    pub id: Option<u32>,
    pub name: String,
    pub kind: SpaceKind,
    /// Allocated bytes. For an unfinished folder, what was measured so far.
    pub bytes: f64,
    /// Files below this entry (merged entries: how many were merged).
    pub items: f64,
    /// The folder's subtree is completely measured.
    pub done: bool,
    /// It can be opened: a finished or in-progress folder with a breakdown.
    pub openable: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, specta::Type)]
pub struct SpaceCrumb {
    pub id: u32,
    pub name: String,
}

/// A folder and its largest entries, sorted largest first, with merges applied.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, specta::Type)]
pub struct SpaceSubtree {
    pub folder: SpaceNode,
    /// From the volume root to this folder, inclusive.
    pub path: Vec<SpaceCrumb>,
    pub children: Vec<SpaceNode>,
    /// While the folder is unfinished: its total not yet attributed to a finished entry.
    /// At the volume root this is measured against the volume's used bytes.
    pub unmeasured_bytes: Option<f64>,
    /// The total the breakdown is drawn against (folder bytes, or used bytes at the root).
    pub total_bytes: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, specta::Type)]
pub struct SpaceFile {
    /// Index for `reveal`.
    pub index: u32,
    pub name: String,
    /// Parent folder relative to the volume root, with "/" separators.
    pub parent: String,
    pub bytes: f64,
}

/// A finished report's summary.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, specta::Type)]
pub struct SpaceReport {
    pub id: u32,
    pub volume: SpaceVolume,
    pub finished_ms: f64,
    pub duration_ms: f64,
    pub items: f64,
    pub used_bytes: f64,
    /// Measured in files on the scanned volume.
    pub files_bytes: f64,
    /// Other volumes in the container.
    pub volumes_bytes: f64,
    /// Used space not attributed to files or volumes; `None` when files exceed it
    /// (APFS clones counted for each copy).
    pub other_bytes: Option<f64>,
    pub unreadable: u32,
    /// The first unreadable folders, relative to the volume root.
    pub unreadable_paths: Vec<String>,
    /// Labels of protected locations that were skipped.
    pub skipped: Vec<String>,
    /// The tree hit its retention cap: deep breakdowns are folded, totals are complete.
    pub truncated: bool,
}

/// What `disk_space_reveal` shows in Finder.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SpaceTarget {
    Node { id: u32 },
    LargestFile { index: u32 },
}
