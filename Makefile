CARGO ?= cargo
CARGO_FLAGS ?= --locked
FEATURES ?=
FEATURE_FLAGS := $(if $(FEATURES),--no-default-features --features "$(FEATURES)")
RELEASE := target/release
BINS := crab-widget crab-hook crab-gui crab-codex

ifeq ($(shell id -u),0)
PREFIX ?= /usr/local
BINDIR ?= $(PREFIX)/bin
THEMEDIR ?= $(PREFIX)/share/crab-on-desk/themes
else
BINDIR ?= $(HOME)/.local/bin
THEMEDIR ?= $(or $(XDG_CONFIG_HOME),$(HOME)/.config)/crab-on-desk/themes
endif

BUILD := build
CLAWD_REPO := https://github.com/rullerzhou-afk/clawd-on-desk.git
CLAWD_COMMIT := 782edefc7ebff70d462f326f1354451d1b86ffa3
CLAWD_ON_DESK ?= $(BUILD)/clawd-on-desk
THEMES := clawd calico cloudling
SIZE := 200
ELECTRON ?= $(shell for version in $$(seq 99 -1 10) ''; do command -v electron$$version && break; done)
HEADLESS := --ozone-platform=headless --ozone-override-screen-size=1024,1024

.PHONY: all build install uninstall themes themes-install themes-uninstall clean

all: build

build:
	$(CARGO) build --release $(CARGO_FLAGS) $(FEATURE_FLAGS) $(addprefix -p ,$(BINS))

install: build
	install -Dm755 -t "$(DESTDIR)$(BINDIR)" $(addprefix $(RELEASE)/,$(BINS))

uninstall:
	rm -f $(addprefix "$(DESTDIR)$(BINDIR)"/,$(BINS))

themes: $(BUILD)/themes.stamp

$(BUILD)/clawd-on-desk:
	git clone --filter=blob:none $(CLAWD_REPO) $@
	git -C $@ checkout --detach $(CLAWD_COMMIT)

GENERATOR_SOURCES := Cargo.lock $(wildcard crab-settings/src/*.rs crab-common/src/*.rs)

$(BUILD)/apng.stamp: scripts/render.js scripts/page.html | $(CLAWD_ON_DESK)
	@test -n "$(ELECTRON)" || { echo "No electron or electronNN on PATH" >&2; exit 1; }
	rm -rf $(BUILD)/apng
	$(ELECTRON) scripts/render.js $(abspath $(CLAWD_ON_DESK)) $(abspath $(BUILD)/apng) $(SIZE) $(THEMES) $(HEADLESS)
	touch $@

$(BUILD)/themes.stamp: $(BUILD)/apng.stamp $(GENERATOR_SOURCES)
	$(CARGO) build --release $(CARGO_FLAGS) -p crab-settings
	rm -rf $(BUILD)/themes
	$(RELEASE)/crab-settings generate $(BUILD)/apng $(BUILD)/themes $(THEMES)
	touch $@

themes-install: themes
	for theme in $(THEMES); do \
		rm -rf "$(DESTDIR)$(THEMEDIR)/$$theme" && \
		install -Dm644 -t "$(DESTDIR)$(THEMEDIR)/$$theme" $(BUILD)/themes/$$theme/* || exit 1; \
	done

themes-uninstall:
	rm -rf $(addprefix "$(DESTDIR)$(THEMEDIR)"/,$(THEMES))

clean:
	rm -rf $(BUILD)
