TARGET = debug
DEBUG ?= 0

ifeq ($(DESTDIR),)
BASE_PATH = $(HOME)/.local/
LIB_PATH = $(BASE_PATH)/share
else
BASE_PATH = $(DESTDIR)/usr/
LIB_PATH = $(BASE_PATH)/lib
endif

LAUNCHER_DIR = $(LIB_PATH)/pop-launcher
SCRIPTS_DIR = $(LAUNCHER_DIR)/scripts
PLUGIN_DIR = $(LAUNCHER_DIR)/plugins
DEFAULT_PLUGINS_BIN = $(PLUGIN_DIR)/pop-launcher-plugins
BIN_DIR = $(BASE_PATH)/bin

PLUGINS=calc desktop_entries files find pop_shell pulse recent scripts terminal web

.PHONY = all clean install uninstall vendor

ifeq ($(DEBUG),0)
	TARGET = release
	ARGS += --release
endif

VENDOR ?= 0
ifneq ($(VENDOR),0)
	ARGS += --frozen --offline
	DESKTOP_ARGS += --frozen --offline
endif

all: extract-vendor
	cargo build -p pop-launcher-plugins $(ARGS)
	cargo build $(ARGS)

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
	for plugin in $(PLUGINS); do \
		dest=$(PLUGIN_DIR)/$${plugin}; \
		mkdir -p $${dest}; \
		install -Dm0644 plugins/src/plugins/$${plugin}/plugin.ron $${dest}/plugin.ron; \
	done

	install -Dm0755 target/$(TARGET)/pop-launcher $(BIN_DIR)/pop-launcher
	install -Dm0755 target/$(TARGET)/pop-launcher-plugins $(DEFAULT_PLUGINS_BIN)

	# Pop Shell Windows plugin
	ln -sf $(DEFAULT_PLUGINS_BIN) $(PLUGIN_DIR)/pop_shell/pop-shell

	# Desktop Entries plugin
	ln -sf $(DEFAULT_PLUGINS_BIN) $(PLUGIN_DIR)/desktop_entries/desktop-entries

	# Find plugin
	ln -sf $(DEFAULT_PLUGINS_BIN) $(PLUGIN_DIR)/find/find

	# Scripts plugin
	ln -sf $(DEFAULT_PLUGINS_BIN) $(PLUGIN_DIR)/scripts/scripts

	# Calculator plugin
	install -Dm0755 plugins/src/plugins/calc/calc.js $(PLUGIN_DIR)/calc
	install -Dm0644 plugins/src/plugins/calc/math.js $(PLUGIN_DIR)/calc

	# Files plugin
	install -Dm0755 plugins/src/plugins/files/files.js $(PLUGIN_DIR)/files

	# Recent plugin
	install -Dm0755 plugins/src/plugins/recent/recent.js $(PLUGIN_DIR)/recent

	# Pulse plugin
	install -Dm0755 plugins/src/plugins/pulse/pulse.js $(PLUGIN_DIR)/pulse

	# Terminal plugin
	install -Dm0755 plugins/src/plugins/terminal/terminal.js $(PLUGIN_DIR)/terminal

	# Web plugin
	install -Dm0755 plugins/src/plugins/web/web.js $(PLUGIN_DIR)/web

	# Scripts
	mkdir -p $(SCRIPTS_DIR)
	for script in $(PWD)/scripts/*; do \
		cp -r $${script} $(SCRIPTS_DIR); \
	done
