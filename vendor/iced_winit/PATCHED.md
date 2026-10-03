iced_winit 0.14.1 from crates.io (MIT, https://github.com/iced-rs/iced), patched for crab-on-desk.

`window::Action::Resize` ignored the size `request_inner_size` applies immediately on Wayland, so
no `Resized` reached iced and it kept rendering the old size into the new viewport (squashed
content on KWin, which sends no configure for a client-side resize). The patch feeds the applied
size back as a `Resized` window event. `[lints.rust] deprecated = "allow"` keeps upstream's
deprecation warnings out of the workspace build. Drop this crate and the `[patch.crates-io]` entry
once upstream fixes it.
