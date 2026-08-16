# The `termproof` CLI as a container image.
#
# Two stages, and the split is the point: the builder carries the Rust
# toolchain and the 1.5 GB of intermediate objects a release build produces,
# and none of it reaches the runtime image. What ships is one statically-linked
# -against-nothing-unusual ELF binary, the shared libraries it already needed,
# and the external programs the tool shells out to. `docker/smoke/run-smoke.sh`
# asserts that no `cargo`, `rustc` or `rustup` survived into the runtime stage,
# so this property is checked on every pull request rather than assumed.
#
# ## Which external programs, and why
#
# TermProof shells out for three things. Working out which belong in an image
# means reading what the shipped CLI can actually reach today (v0.3.2):
#
#   rsvg-convert  `evidence::screenshot::ScreenshotRenderer` rasterises its SVG
#                 through it, at the hard-coded path /usr/bin/rsvg-convert —
#                 which is exactly where Debian's librsvg2-bin puts it.
#                 `evidence::cast_video` uses it per frame. INCLUDED: it is a
#                 20 MB dependency standing behind every still the evidence
#                 pipeline can produce, and there is no fallback that draws
#                 glyphs (`render_png` paints blocks, not letters).
#
#   ffmpeg        The encoder both video backends share:
#                 `evidence::video::AggFfmpegBackend` resolves it from PATH (or
#                 TERM_PROOF_FFMPEG), and `evidence::cast_video` calls it at the
#                 hard-coded path /usr/local/bin/ffmpeg — which is *not* where
#                 Debian puts it, hence the symlink below. INCLUDED: without it
#                 no video path exists at all, and with it `cast_video` is
#                 complete, since that backend renders its own frames and needs
#                 no `agg`.
#
#   agg           Serves `AggFfmpegBackend` and nothing else. EXCLUDED. Debian
#                 does not package it, so it would mean a second Rust builder
#                 stage compiling a third-party crate from git on every image
#                 build, to serve the one video backend that `cast_video`
#                 already covers without it. What breaks: a caller selecting
#                 the `agg_ffmpeg` backend gets that backend's own diagnostic —
#                 "agg not found in PATH and TERM_PROOF_AGG not set" — which
#                 names the fix. Set TERM_PROOF_AGG to a mounted binary, or
#                 use the cast backend.
#
# Read the CLI honestly and none of the three is reachable from it *yet*:
# `termproof run --video` prints "accepted but not implemented yet" and the
# run path writes text, a cast and reports. The two above are here so that the
# published image does not silently regress the day those paths are wired, and
# because the library in this image is a usable dependency in its own right.
# The pull-request smoke run exercises both through a real recipe; see
# docker/smoke/run-smoke.sh.
#
# ## Fonts
#
# `terminal::attributed::FONT_STACK` is "Noto Sans Mono, Liberation Mono,
# monospace". rsvg-convert resolves that through fontconfig, and a missing
# font is not an error — it is a silently blank panel. fonts-liberation2
# supplies Liberation Mono, matching the second entry exactly, for about 1 MB.

FROM rust:1.96.0-slim-bookworm@sha256:4732ca96fd086cb9be682050c3f0176288eebaac2b80aa2bcefccfaf198e1950 AS builder

WORKDIR /src

# The workspace's own toolchain pin. The base image already carries 1.96.0, so
# rustup has nothing to fetch; the file is copied first so that a change to it
# is a visible cache miss rather than a silent toolchain swap.
COPY rust-toolchain.toml ./
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates

# --locked, so the image is built from the committed dependency graph and an
# image build can never be the thing that updates Cargo.lock. Symbols are
# dropped by rustc rather than by `strip`, which keeps this independent of
# whether the base image happens to ship binutils.
RUN RUSTFLAGS="-C strip=symbols" cargo build --locked --release -p termproof-cli


FROM debian:bookworm-slim@sha256:abd67ffcfa541b485a3dff59865ab629aa048a6c613e639d36e7456b0b229241

LABEL org.opencontainers.image.title="termproof" \
      org.opencontainers.image.description="Evidence-first verification for TUI and terminal applications (Rust implementation)" \
      org.opencontainers.image.source="https://github.com/md-mt/termproof-rust" \
      org.opencontainers.image.licenses="MIT"

ENV TERM=xterm-256color

RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        ca-certificates \
        ffmpeg \
        fonts-liberation2 \
        librsvg2-bin \
    && rm -rf /var/lib/apt/lists/* \
    # `evidence::cast_video` looks for ffmpeg at /usr/local/bin/ffmpeg and
    # nothing resolves that path for it — it is a constant, not a PATH lookup.
    # Debian installs to /usr/bin, so without this link the cast video backend
    # fails on an image that plainly has ffmpeg in it.
    && ln -s /usr/bin/ffmpeg /usr/local/bin/ffmpeg

COPY --from=builder /src/target/release/termproof /usr/local/bin/termproof
COPY docker/smoke /opt/termproof/smoke

# Fail the build rather than publish an image whose binary cannot start. This
# is the cheap check; the real one is docker/smoke/run-smoke.sh, which the
# workflow runs against the built image on every pull request.
RUN termproof --version \
    && rsvg-convert --version \
    && ffmpeg -version >/dev/null \
    && test -x /opt/termproof/smoke/run-smoke.sh

# Deliberately root. The normal invocation bind-mounts a host directory and
# writes evidence into it (`--out`), and a fixed non-root UID inside the image
# cannot match the host user's, so it would turn the common case into a
# permission error. Callers who want another identity can pass `--user`.
WORKDIR /workspace
ENTRYPOINT ["termproof"]
CMD ["--help"]
