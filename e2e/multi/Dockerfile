# The multi-computer end-to-end tests run here: Ubuntu packages WebKit's WebDriver, which
# drives the real app window (Arch's WebKit is built without it). Same tool versions as the
# project, through mise.
FROM ubuntu:24.04
ENV DEBIAN_FRONTEND=noninteractive
RUN apt-get update && apt-get install -y --no-install-recommends \
      build-essential ca-certificates curl file git pkg-config libssl-dev \
      libwebkit2gtk-4.1-dev webkit2gtk-driver libsoup-3.0-dev libjavascriptcoregtk-4.1-dev \
      libgtk-3-dev librsvg2-dev libayatana-appindicator3-dev \
      xvfb xauth dbus dbus-x11 fonts-noto-core \
    && rm -rf /var/lib/apt/lists/*
RUN sh -c "$(curl -fsLS get.chezmoi.io)" -- -b /usr/local/bin
RUN curl -fsSL https://mise.run | MISE_INSTALL_PATH=/usr/local/bin/mise sh
ENV MISE_DATA_DIR=/opt/mise
WORKDIR /setup
COPY .mise.toml ./
RUN mise trust /setup/.mise.toml && mise install
RUN mise x -- cargo install tauri-driver --locked
# The tests run as the person's own user (so files written into the checkout stay theirs):
# the tools and the build cache must be writable by anyone.
RUN chmod -R a+rwX /opt/mise && mkdir -p /cache/target && chmod 777 /cache/target
WORKDIR /app
