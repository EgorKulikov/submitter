# Extends the official judgehost image to install Rust into the chroot used
# for judging. DOMjudge ships a Rust language definition (id `rs`) but does
# not include rustc in the default chroot; without this layer, submissions
# in Rust would fail to compile.

FROM docker.io/domjudge/judgehost:latest

# Pull resolv.conf into the chroot so apt can reach the package mirrors at
# build time, then install rustc/cargo. The chroot already has a working
# sources.list from the upstream debootstrap.
RUN cp /etc/resolv.conf /chroot/domjudge/etc/resolv.conf \
 && chroot /chroot/domjudge bash -c '\
        export DEBIAN_FRONTEND=noninteractive && \
        apt-get update && \
        apt-get install -y --no-install-recommends rustc cargo && \
        apt-get clean && \
        rm -rf /var/lib/apt/lists/*' \
 && rustc_version="$(chroot /chroot/domjudge rustc --version 2>&1)" \
 && echo "[ok] $rustc_version installed in chroot"
