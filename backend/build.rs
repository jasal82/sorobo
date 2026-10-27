// Copyright (C) 2026 Johannes Asal
// SPDX-License-Identifier: AGPL-3.0-or-later
// See LICENSE in the project root for the full license and warranty disclaimer.

//! Embeds icon and version information into the Windows executables.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=assets/icons/sorobo.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let mut res = winresource::WindowsResource::new();
    // Resource id 1 – the desktop window loads its icon from here.
    res.set_icon_with_id("assets/icons/sorobo.ico", "1");
    res.set("ProductName", "Sorobo");
    res.set("FileDescription", "Sorobo – Buchhaltung");
    res.set("CompanyName", "Sorobo");
    res.set("LegalCopyright", "Copyright (C) 2026 Johannes Asal");
    if let Err(e) = res.compile() {
        // A missing resource compiler only costs the icon, not the build.
        println!("cargo:warning=Programmsymbol nicht eingebunden: {e}");
    }
}
