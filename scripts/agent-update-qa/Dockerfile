FROM debian:bookworm-slim
ENV container=docker
RUN apt-get update && apt-get install -y --no-install-recommends systemd systemd-sysv dbus dbus-user-session python3 ca-certificates && rm -rf /var/lib/apt/lists/*
RUN useradd --create-home --uid 1000 kelvo && mkdir -p /var/lib/systemd/linger && touch /var/lib/systemd/linger/kelvo
STOPSIGNAL SIGRTMIN+3
CMD ["/sbin/init"]
