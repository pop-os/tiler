TARGET = debug
DEBUG ?= 0

BASE_PATH = $(DESTDIR)/usr
BIN_DIR = $(BASE_PATH)/bin
BIN = $(BIN_DIR)/pop-tiler

.PHONY = all clean install uninstall vendor

ifeq ($(DEBUG),0)
	TARGET = release
	ARGS += --release
endif

VENDOR ?= 0
ifneq ($(VENDOR),0)
	ARGS += --frozen --offline
endif

all: extract-vendor
	cargo build -p pop-tiler-ipc $(ARGS)

clean:
	cargo clean

distclean:
	rm -rf .cargo vendor vendor.tar target

vendor:
	mkdir -p .cargo
	cargo vendor --sync plugins/Cargo.toml | head -n -1 > .cargo/config
	echo 'directory = "vendor"' >> .cargo/config
	tar pcf vendor.tar vendor
	rm -rf vendor

extract-vendor:
ifeq ($(VENDOR),1)
	rm -rf vendor; tar pxf vendor.tar
endif

install:
	install -Dm0755 target/$(TARGET)/pop-tiler-ipc $(BIN)

uninstall:
	rm $(BIN)