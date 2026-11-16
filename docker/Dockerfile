FROM rust:1.85-bookworm

RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        build-essential \
        gdb \
        make \
        binutils-m68k-linux-gnu \
        gcc-m68k-linux-gnu \
    && rm -rf /var/lib/apt/lists/*

RUN rustup component add rustfmt

RUN ln -sf /usr/bin/m68k-linux-gnu-as /usr/local/bin/m68k-unknown-elf-as \
    && ln -sf /usr/bin/m68k-linux-gnu-ld /usr/local/bin/m68k-unknown-elf-ld
