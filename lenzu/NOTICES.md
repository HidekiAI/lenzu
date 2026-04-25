# Third-Party Notices for Lenzu

Lenzu (the client binary) is licensed MIT.  This document acknowledges
third-party software that Lenzu uses or links against, in compliance with
the attribution clauses of those projects' licenses.

This file is shown in the "About" view of the in-app Help dialog
(Shift+H → About) and is installed at `/usr/share/doc/lenzu/NOTICES.md`.

The companion HUD package (`lenzu-hud`, MIT) maintains its own notices
under `/usr/share/doc/lenzu-hud/`.  The optional DBNet model package
(`lenzu-models-dbnet`, AGPL-3.0) maintains its own copyright under
`/usr/share/doc/lenzu-models-dbnet/`.

---

## Japanese Morphological Analysis

### MeCab
Copyright (c) 2001-2008, Taku Kudo
Copyright (c) 2004-2008, Nippon Telegraph and Telephone Corporation
Tri-licensed under GPL, LGPL, or BSD-3-Clause.  Lenzu uses it under the
BSD branch via dynamic linking against `libmecab2`.

  https://taku910.github.io/mecab/

### MeCab IPADIC (`mecab-ipadic-utf8`)
Copyright Nara Institute of Science and Technology and the
Mainichi Newspapers Company.  Released under a BSD-style license.

---

## GUI / Rendering (System Libraries, Dynamically Linked)

The following libraries are LGPL-2.1+ (or compatible) and are linked
dynamically.  No bundled copies; they are provided by the user's
distribution via `apt`.  Source is available from the distribution.

  - GTK+ 3 — https://www.gtk.org/
  - Cairo — https://www.cairographics.org/
  - Pango — https://pango.gnome.org/
  - GDK-Pixbuf — https://gitlab.gnome.org/GNOME/gdk-pixbuf
  - GLib — https://gitlab.gnome.org/GNOME/glib

### Fonts
Lenzu does not bundle fonts.  CJK rendering relies on the system-
installed `fonts-noto-cjk` (SIL Open Font License 1.1) and
`fonts-ipafont-gothic` (IPA Font License v1.0).

  - Noto CJK — © Google Inc. / Adobe Inc., OFL-1.1
  - IPA Fonts — © Information-technology Promotion Agency, IPA-1.0

---

## Networking / Cryptography

### OpenSSL (libssl3)
Copyright the OpenSSL Project.  Apache License 2.0 (OpenSSL 3.x).
Linked dynamically via the `reqwest` Rust crate.

  https://www.openssl.org/

---

## Machine Learning Runtime

### ONNX Runtime
Copyright (c) Microsoft Corporation.  MIT License.
Used (when the `onnx` Cargo feature is enabled) via the `ort` Rust crate
for DBNet text detection inference.

  https://onnxruntime.ai/

---

## Electron HUD (`lenzu-hud`, separately packaged)

The HUD overlay is an Electron application; full attribution is shipped
under `/usr/share/doc/lenzu-hud/`.  Electron itself is MIT-licensed and
includes Chromium (BSD-style), V8 (BSD-3-Clause), and Node.js (MIT).

---

## Rust Crates

The Lenzu binary depends on a large number of Rust crates.  All direct
dependencies are MIT, Apache-2.0, or dual-licensed MIT/Apache-2.0.
For a machine-readable list, run from the source repository:

    cargo install cargo-license
    cargo license --json

Notable direct dependencies:

  - `gtk`, `gdk`, `glib`, `cairo-rs`, `pango`, `pangocairo`, `gdk-pixbuf`
    (gtk-rs project, MIT) — © The gtk-rs Project Developers
  - `tokio` (MIT) — © Tokio Contributors
  - `reqwest` (MIT/Apache-2.0) — © Sean McArthur
  - `serde`, `serde_json` (MIT/Apache-2.0) — © David Tolnay et al.
  - `image` (MIT/Apache-2.0) — © The image-rs Developers
  - `x11rb` (MIT/Apache-2.0)
  - `arboard` (MIT/Apache-2.0)
  - `mecab` Rust binding (MIT) — © Cristian Kubis
  - `mecab-furigana-rs` (MIT)
  - `manga-ocr-rs` (MIT)
  - `jp_detect` (MIT)
  - `isolang` (MIT/Apache-2.0)
  - `chrono` (MIT/Apache-2.0)
  - `base64` (MIT/Apache-2.0)
  - `anyhow` (MIT/Apache-2.0)
  - `async-channel` (MIT/Apache-2.0)
  - `libc` (MIT/Apache-2.0)

---

## Optional: DBNet Text Detection Model

The DBNet ONNX model — when installed via the separate
`lenzu-models-dbnet` package — is licensed **AGPL-3.0** by upstream
StabRise and is *not* bundled with the MIT `lenzu` package.  See:

    /usr/share/doc/lenzu-models-dbnet/copyright

---

For corrections or additions to this notice, please open an issue at
https://github.com/hidekiai/lenzu
