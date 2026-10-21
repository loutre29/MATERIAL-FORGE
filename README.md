# Material Forge

A clean-room PBR 3D texture painter written in Rust (wgpu + egui), modelled
on the workflow of Adobe Substance 3D Painter. No Adobe code, shader, preset
or asset is used: behaviour is reproduced from public documentation and use
of the application as a black box.

`CONTEXT.md` is the developer guide (features, layout, conventions, tests).

## Build

```
cargo build --release
target\release\material_forge.exe
```

Tests: `--selftest <dir>`, `--uitest <dir>`, `--uicrawl <dir>` (see CONTEXT.md).

## Installer

With Inno Setup 6 installed:

```
cargo build --release
ISCC.exe installer\material_forge.iss
```

The installer lands in `dist\`.
