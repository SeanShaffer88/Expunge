//! Expunge core library: snapshots, diffs, precise uninstall plans, backups,
//! leftover scanning, startup management, cleanup, and reports.
//!
//! Copyright (C) 2026 Sean Shaffer
//!
//! This program is free software: you can redistribute it and/or modify it
//! under the terms of the GNU General Public License as published by the Free
//! Software Foundation, either version 3 of the License, or (at your option)
//! any later version. See LICENSE for details.

pub mod autoruns;
pub mod backup;
pub mod cache;
pub mod cleaner;
pub mod diskmap;
pub mod duplicates;
pub mod autowatch;
pub mod boot;
pub mod health;
pub mod live;
pub mod msix;
pub mod reputation;
pub mod schedule;
pub mod scripts;
pub mod features;
pub mod i18n;
pub mod leftovers;
pub mod lint;
pub mod model;
pub mod noise;
pub mod orphans;
pub mod platform;
pub mod programs;
pub mod repackage;
pub mod report;
pub mod revert;
pub mod settings;
pub mod shell;
pub mod silent;
pub mod slimmer;
pub mod snapshot;
pub mod startup;
pub mod store;
pub mod transfer;
pub mod trust;
pub mod tweaks;
pub mod updates;
pub mod util;
pub mod winget;

pub const APP_NAME: &str = "Expunge";
pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");
pub const APP_AUTHOR: &str = "Sean Shaffer";
