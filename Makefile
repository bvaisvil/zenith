.PHONY: all base clean install uninstall linux-static

DESTDIR =

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
	install -D -m 755 build/zenith.base $(DESTDIR)/usr/bin/zenith.base
	install -D -m 755 build/zenith.nvidia $(DESTDIR)/usr/bin/zenith.nvidia
	install -D -m 755 assets/zenith.sh $(DESTDIR)/usr/bin/zenith
	install -D -m 644 assets/zenith.png $(DESTDIR)/usr/share/pixmaps/zenith.png
	install -D -m 644 assets/zenith.desktop $(DESTDIR)/usr/share/applications/zenith.desktop

uninstall: clean
	rm -f $(DESTDIR)/usr/bin/zenith.base $(DESTDIR)/usr/bin/zenith.nvidia $(DESTDIR)/usr/bin/zenith
	rm -f $(DESTDIR)/usr/share/pixmaps/zenith.png $(DESTDIR)/usr/share/applications/zenith.desktop

linux-static: clean
	CC_x86_64_unknown_linux_musl="x86_64-linux-musl-gcc" cargo build --release --target=x86_64-unknown-linux-musl
	install -D -m 755 target/release/zenith linux.static/zenith.base
	cargo clean
	CC_x86_64_unknown_linux_musl="x86_64-linux-musl-gcc" cargo build --release --target=x86_64-unknown-linux-musl --features nvidia
	install -D -m 755 target/release/zenith linux.static/zenith.nvidia
	install -D -m 755 assets/zenith.sh linux.static/zenith
	tar -C linux.static -c -z -v -f zenith.x86_64-unknown-linux-musl.tgz .
	sha256sum zenith.x86_64-unknown-linux-musl.tgz | cut -d' ' -f1 > zenith.x86_64-unknown-linux-musl.tgz.sha256
