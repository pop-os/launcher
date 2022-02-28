TARGET = debug
DEBUG ?= 0

ifeq ($(DESTDIR),)
BASE_PATH = $(HOME)/.local
LIB_PATH = $(BASE_PATH)/share
else
BASE_PATH = $(DESTDIR)/usr
LIB_PATH = $(BASE_PATH)/lib
endif

LAUNCHER_DIR = $(LIB_PATH)/pop-launcher
SCRIPTS_DIR = $(LAUNCHER_DIR)/scripts
PLUGIN_DIR = $(LAUNCHER_DIR)/plugins
BIN_DIR = $(BASE_PATH)/bin
BIN = $(BIN_DIR)/pop-launcher

PLUGINS=calc desktop_entries files find pop_shell pulse recent scripts terminal web

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
	cargo build -p pop-launcher-bin $(ARGS)

clean:
	cargo clean

distclean:
	rm -rf .cargo vendor vendor.tar target

vendor:
	mkdir -p .cargo
	cargo vendor --sync plugins/Cargo.toml --sync service/Cargo.toml | head -n -1 > .cargo/config
	echo 'directory = "vendor"' >> .cargo/config
	tar pcf vendor.tar vendor
	rm -rf vendor

extract-vendor:
ifeq ($(VENDOR),1)
	rm -rf vendor; tar pxf vendor.tar
endif

install:
	for plugin in $(PLUGINS); do \
		dest=$(PLUGIN_DIR)/$${plugin}; \
		mkdir -p $${dest}; \
		install -Dm0644 plugins/src/$${plugin}/*.ron $${dest}; \
	done

	install -Dm0755 target/$(TARGET)/pop-launcher-bin $(BIN)

	# Pop Shell Windows plugin
	ln -sf $(BIN) $(PLUGIN_DIR)/pop_shell/pop-shell

	# Desktop Entries plugin
	ln -sf $(BIN) $(PLUGIN_DIR)/desktop_entries/desktop-entries

	# Find plugin
	ln -sf $(BIN) $(PLUGIN_DIR)/find/find

	# Scripts plugin
	ln -sf $(BIN) $(PLUGIN_DIR)/scripts/scripts

	# Web plugin
	ln -sf $(BIN) $(PLUGIN_DIR)/web/web

	# Calculator plugin
	ln -sf $(BIN) $(PLUGIN_DIR)/calc/calc

	# Files plugin
	ln -sf $(BIN) $(PLUGIN_DIR)/files/files

	# Recent plugin
	ln -sf $(BIN) $(PLUGIN_DIR)/recent/recent

	# Pulse plugin
	ln -sf $(BIN) $(PLUGIN_DIR)/pulse/pulse

	# Terminal plugin
	ln -sf $(BIN) $(PLUGIN_DIR)/terminal/terminal

	# Scripts
	mkdir -p $(SCRIPTS_DIR)
	for script in $(PWD)/scripts/*; do \
		cp -r $${script} $(SCRIPTS_DIR); \
	done

release:
	sed -i "s/^version.*/version = \"${RELEASE}\"/g" Cargo.toml
	sed -i "s/^version.*/version = \"${RELEASE}\"/g" bin/Cargo.toml
	sed -i "s/^version.*/version = \"${RELEASE}\"/g" plugins/Cargo.toml
	sed -i "s/^version.*/version= \"${RELEASE}\"/g" service/Cargo.toml
	cargo update
