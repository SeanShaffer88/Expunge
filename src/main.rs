// Expunge — install monitor and complete uninstaller for Windows.
// Copyright (C) 2026 Sean Shaffer
//
// This program is free software: you can redistribute it and/or modify it
// under the terms of the GNU General Public License as published by the Free
// Software Foundation, either version 3 of the License, or (at your option)
// any later version. This program is distributed WITHOUT ANY WARRANTY.
// See the LICENSE file for details.

// No console window for the GUI; the CLI attaches to the parent console.
#![cfg_attr(windows, windows_subsystem = "windows")]

mod cli;
mod gui;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if !args.is_empty() {
        #[cfg(windows)]
        expunge_core::platform::windows::attach_parent_console();
        expunge_core::i18n::set_language("");
        std::process::exit(cli::run(args));
    }
    if let Err(e) = gui::run() {
        #[cfg(windows)]
        {
            let msg = format!("Expunge could not start its window: {e}\n\nThis usually means the graphics driver is missing or broken.");
            let _ = expunge_core::platform::windows::message_box("Expunge", &msg);
        }
        eprintln!("{e}");
        std::process::exit(1);
    }
}
