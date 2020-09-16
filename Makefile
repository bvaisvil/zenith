.PHONY: all base clean install uninstall linux-static

DESTDIR =
PREFIX = /usr/local

all: base
	cargo clean
	RUSTFLAGS="-C link-arg=-s" cargo build --release --features nvidia
	rm -f build/zenith.nvidia
	install -D -m 755 target/release/zenith build/zenith.nvidia

base: clean
	RUSTFLAGS="-C link-arg=-s" cargo build --release
	rm -f build/zenith.base
	install -D -m 755 target/release/zenith build/zenith.base

clean:
	cargo clean
	rm -rf build linux.static

install:
	install -D -m 755 build/zenith.base $(DESTDIR)$(PREFIX)/lib/zenith/base/zenith
	install -D -m 755 build/zenith.nvidia $(DESTDIR)$(PREFIX)/lib/zenith/nvidia/zenith
	install -D -m 755 assets/zenith.sh $(DESTDIR)$(PREFIX)/bin/zenith
	sed -i 's,PREFIX=/usr/local,PREFIX=$(PREFIX),' $(DESTDIR)$(PREFIX)/bin/zenith
	install -D -m 644 assets/zenith.png $(DESTDIR)$(PREFIX)/share/pixmaps/zenith.png
	install -D -m 644 assets/zenith.desktop $(DESTDIR)$(PREFIX)/share/applications/zenith.desktop

uninstall: clean
	rm -f $(DESTDIR)$(PREFIX)/bin/zenith.base $(DESTDIR)$(PREFIX)/bin/zenith.nvidia $(DESTDIR)$(PREFIX)/bin/zenith
	rm -f $(DESTDIR)$(PREFIX)/share/pixmaps/zenith.png $(DESTDIR)$(PREFIX)/share/applications/zenith.desktop

linux-static: clean
	CC_x86_64_unknown_linux_musl="x86_64-linux-musl-gcc" cargo build --release --target=x86_64-unknown-linux-musl
	install -D -m 755 target/release/zenith linux.static/zenith/base/zenith
	cargo clean
	CC_x86_64_unknown_linux_musl="x86_64-linux-musl-gcc" cargo build --release --target=x86_64-unknown-linux-musl --features nvidia
	install -D -m 755 target/release/zenith linux.static/zenith/nvidia/zenith
	install -D -m 755 assets/zenith-static.sh linux.static/zenith
	tar -C linux.static -c -z -v -f zenith.x86_64-unknown-linux-musl.tgz .
	sha256sum zenith.x86_64-unknown-linux-musl.tgz | cut -d' ' -f1 > zenith.x86_64-unknown-linux-musl.tgz.sha256
