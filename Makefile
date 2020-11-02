.PHONY: platform all base nvidia clean install install-base install-nvidia install-both install-desktop uninstall linux-static

DESTDIR =
PREFIX = /usr/local
CARGO_TARGET =
TARGET_TYPE = dynamic
CCFLAGS =
TARGET_BUILDDIR = release
UNAME := $(shell uname)

ifeq ($(UNAME), Linux)
  BUILD_NVIDIA = true
else
  BUILD_NVIDIA = false
endif

STATIC_TARGET = x86_64-unknown-linux-musl
CC_STATIC_TARGET = x86_64_unknown_linux_musl
STATIC_DIR = build/static-bundle
STATIC_EXEC_DIR = $(STATIC_DIR)/zenith-exec


platform:
	@if [ $(BUILD_NVIDIA) = true ] && sh assets/zenith-libnvidia-detect.sh; then \
	  $(MAKE) nvidia; \
	else \
	  $(MAKE) base; \
	fi

all: base
	@if [ $(BUILD_NVIDIA) = true ] && sh assets/zenith-libnvidia-detect.sh; then \
	  $(MAKE) clean nvidia; \
	fi

base:
	$(CCFLAGS) RUSTFLAGS="-C link-arg=-s" cargo build --release $(CARGO_TARGET)
	mkdir -p build/$(TARGET_TYPE)
	rm -f build/$(TARGET_TYPE)/zenith.base
	install -m 755 target/$(TARGET_BUILDDIR)/zenith build/$(TARGET_TYPE)/zenith.base

nvidia:
	@for path in `echo $$LD_LIBRARY_PATH | sed 's/:/ /g'`; do \
	  libpaths="$$libpaths -L$$path"; \
	done
	$(CCFLAGS) RUSTFLAGS="$$libpaths -C link-arg=-s" cargo build --release $(CARGO_TARGET) --features nvidia
	mkdir -p build/$(TARGET_TYPE)
	rm -f build/$(TARGET_TYPE)/zenith.nvidia
	install -m 755 target/$(TARGET_BUILDDIR)/zenith build/$(TARGET_TYPE)/zenith.nvidia

clean:
	cargo clean
	rm -rf build
	rm -f zenith.$(STATIC_TARGET).tgz*

install:
	mkdir -p "$(DESTDIR)$(PREFIX)/bin"
	@if [ -x build/$(TARGET_TYPE)/zenith.nvidia ]; then \
	  if [ -x build/$(TARGET_TYPE)/zenith.base ]; then \
	    $(MAKE) install-both; \
	  else \
	    $(MAKE) install-nvidia; \
	  fi \
	else \
	  $(MAKE) install-base; \
	fi
	@if [ $(UNAME) = "Linux" ]; then \
	  $(MAKE) install-desktop; \
	fi

install-base:
	install -m 755 build/$(TARGET_TYPE)/zenith.base "$(DESTDIR)$(PREFIX)/bin/zenith"

install-nvidia:
	install -m 755 build/$(TARGET_TYPE)/zenith.nvidia "$(DESTDIR)$(PREFIX)/bin/zenith"

install-both:
	mkdir -p "$(DESTDIR)$(PREFIX)/lib/zenith/base" "$(DESTDIR)$(PREFIX)/lib/zenith/nvidia"
	install -m 755 build/$(TARGET_TYPE)/zenith.base "$(DESTDIR)$(PREFIX)/lib/zenith/base/zenith"
	install -m 755 build/$(TARGET_TYPE)/zenith.nvidia "$(DESTDIR)$(PREFIX)/lib/zenith/nvidia/zenith"
	install -m 755 assets/zenith-libnvidia-detect.sh "$(DESTDIR)$(PREFIX)/lib/zenith/zenith-libnvidia-detect"
	install -m 755 assets/zenith.sh "$(DESTDIR)$(PREFIX)/bin/zenith"
	sed -i 's,PREFIX=/usr/local,PREFIX=$(PREFIX),' "$(DESTDIR)$(PREFIX)/bin/zenith"

install-desktop:
	mkdir -p "$(DESTDIR)$(PREFIX)/share/applications" "$(DESTDIR)$(PREFIX)/share/pixmaps"
	install -m 644 assets/zenith.png "$(DESTDIR)$(PREFIX)/share/pixmaps/zenith.png"
	install -m 644 assets/zenith.desktop "$(DESTDIR)$(PREFIX)/share/applications/zenith.desktop"

uninstall:
	rm -rf "$(DESTDIR)$(PREFIX)/lib/zenith" "$(DESTDIR)$(PREFIX)/bin/zenith"
	rm -f "$(DESTDIR)$(PREFIX)/share/pixmaps/zenith.png" "$(DESTDIR)$(PREFIX)/share/applications/zenith.desktop"
	@rmdir "$(DESTDIR)$(PREFIX)/bin" "$(DESTDIR)$(PREFIX)/lib" "$(DESTDIR)$(PREFIX)/share/applications" "$(DESTDIR)$(PREFIX)/share/pixmaps" "$(DESTDIR)$(PREFIX)/share" 2>/dev/null || /bin/true

linux-static-init:
	rustup target add $(STATIC_TARGET)

linux-static: CARGO_TARGET = --target=$(STATIC_TARGET)
linux-static: TARGET_TYPE = static
linux-static: CCFLAGS = CC_$(CC_STATIC_TARGET)=musl-gcc
linux-static: TARGET_BUILDDIR = $(STATIC_TARGET)/release
# NVIDIA driver does not ship with static libraries
linux-static: BUILD_NVIDIA = false
linux-static: linux-static-init all
	mkdir -p $(STATIC_DIR)
	@if [ -x build/$(TARGET_TYPE)/zenith.nvidia ]; then \
	  mkdir -p $(STATIC_EXEC_DIR)/base $(STATIC_EXEC_DIR)/nvidia; \
	  install -m 755 build/$(TARGET_TYPE)/zenith.base $(STATIC_EXEC_DIR)/base/zenith; \
	  install -m 755 build/$(TARGET_TYPE)/zenith.nvidia $(STATIC_EXEC_DIR)/nvidia/zenith; \
	  install -m 755 assets/zenith-libnvidia-detect.sh $(STATIC_EXEC_DIR)/zenith-libnvidia-detect; \
	  install -m 755 assets/zenith-static.sh $(STATIC_DIR)/zenith; \
	else \
	  install -m 755 build/$(TARGET_TYPE)/zenith.base $(STATIC_DIR)/zenith; \
	fi
	tar -C $(STATIC_DIR) -c -z -v -f zenith.$(STATIC_TARGET).tgz .
	sha256sum zenith.$(STATIC_TARGET).tgz | cut -d' ' -f1 > zenith.$(STATIC_TARGET).tgz.sha256
