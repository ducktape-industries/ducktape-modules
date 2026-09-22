# modules — the wasm32 gates.
CARGO ?= cargo
WASM_OPT ?= wasm-opt

# What a program links: abi and guest build for wasm32 with nothing else.
PROGRAM_LINKABLE := abi guest

# The boot set: its own wasm32 workspace, its bytes committed beside it.
SYSTEM := crates/modules/system

# The ducktape checkout the probe fixture the `modules` suite seats is copied
# from (crates/kernel/fixtures, `make kernel-fixtures` there).
DUCKTAPE ?= ../ducktape

# The app programs, by manifest path: chat-program is its own wasm32 workspace
# (the view links `chat`, never a program), forge a root member whose wasm
# entry is wasm32-gated.
PROGRAMS := crates/app/chat-program crates/app/forge

# Views are wasm32 cdylibs. Chat rides its program; Settings rides the registry.
VIEWS := chat-view members-view node-view explorer-view settings-view

# What a wasm32 view may link. A crate a view links must never reach the
# signing/identity graph (blst does not build for wasm32, and a view has no
# business holding keys). `modules` is here because the system views read the
# boot set's contracts: its signing deps are dev-only, and `-e normal` below
# is what says so.
VIEW_LINKABLE := ducklink view-wire view-guest design modules settings-view
VIEW_FORBIDDEN := blst commonware-cryptography wasm-bindgen js-sys web-sys

.PHONY: program-wasm-check wasm-programs probe-fixture wasm-views view-wasm-check

## builds abi and guest for wasm32-unknown-unknown.
program-wasm-check:
	@for crate in $(PROGRAM_LINKABLE); do \
	  $(CARGO) build --target wasm32-unknown-unknown -p $$crate || exit 1; \
	done; \
	echo "abi and guest build for wasm32"

# Cargo uses this directory for all workspaces, including the standalone programs.
BUILD_TARGET := $(abspath $(or $(CARGO_TARGET_DIR),target))
PACK_DIR := $(CURDIR)/target/pack/rebuilt
PACKER := $(BUILD_TARGET)/debug/view-pack

.PHONY: wasm-modules wasm-modules-check wasm-modules-build

## Build every program and view without modifying committed artifacts.
wasm-modules-build: wasm-views
	$(CARGO) build -p view-pack --target-dir $(BUILD_TARGET)
	$(CARGO) build --manifest-path $(SYSTEM)/Cargo.toml \
	  --target-dir $(BUILD_TARGET) --target wasm32-unknown-unknown --release
	@for p in $(PROGRAMS); do \
	  $(CARGO) build --manifest-path $$p/Cargo.toml --target-dir $(BUILD_TARGET) --target wasm32-unknown-unknown --release || exit 1; \
	done
	@mkdir -p $(PACK_DIR)
	@for name in module_registry valset identity chat_program forge; do \
	  cp $(BUILD_TARGET)/wasm32-unknown-unknown/release/$$name.wasm $(PACK_DIR)/$$name.wasm || exit 1; \
	done
	$(PACKER) $(PACK_DIR)/chat_program.wasm $(BUILD_TARGET)/wasm32-unknown-unknown/release/chat_view.wasm $(PACK_DIR)/chat_program.wasm
	$(PACKER) $(PACK_DIR)/module_registry.wasm $(BUILD_TARGET)/wasm32-unknown-unknown/release/settings_view.wasm $(PACK_DIR)/module_registry.wasm

## Commit these bytes as the founding file's program code, including its view.
wasm-modules: wasm-modules-build
	@mkdir -p crates/app/wasm
	@for name in module_registry valset identity; do cp $(PACK_DIR)/$$name.wasm $(SYSTEM)/wasm/$$name.wasm || exit 1; done
	@for name in chat_program forge; do cp $(PACK_DIR)/$$name.wasm crates/app/wasm/$$name.wasm || exit 1; done

wasm-programs: wasm-modules

## Report every stale artifact in one run, including program-only drift.
wasm-modules-check: wasm-modules-build
	@stale=0; \
	for entry in module_registry:$(SYSTEM)/wasm valset:$(SYSTEM)/wasm identity:$(SYSTEM)/wasm chat_program:crates/app/wasm forge:crates/app/wasm; do \
	  name=$${entry%%:*}; file=$${entry#*:}/$$name.wasm; \
	  if ! cmp -s $(PACK_DIR)/$$name.wasm $$file; then echo "stale: $$file"; stale=1; fi; \
	done; \
	for name in chat_program module_registry; do \
	  $(PACKER) --strip $(PACK_DIR)/$$name.wasm $(PACK_DIR)/$$name.stripped.wasm || exit 1; \
	  cmp $(PACK_DIR)/$$name.stripped.wasm $(BUILD_TARGET)/wasm32-unknown-unknown/release/$$name.wasm || exit 1; \
	done; \
	test $$stale -eq 0

## refreshes the probe fixture the `modules` suite seats as the authority,
## from the ducktape checkout at $(DUCKTAPE).
probe-fixture:
	cp $(DUCKTAPE)/crates/kernel/fixtures/wasm/fixture_probe.wasm crates/modules/tests/

## builds every view for wasm32 under target/wasm32-unknown-unknown/release/.
wasm-views:
	@for v in $(VIEWS); do \
	  $(CARGO) build --release --target wasm32-unknown-unknown -p $$v || exit 1; \
	  artifact="$${CARGO_TARGET_DIR:-target}/wasm32-unknown-unknown/release/$$(echo $$v | tr - _).wasm"; \
	  WASM_OPT="$(WASM_OPT)" tools/optimize-view.sh "$$artifact" || exit 1; \
	  python3 tools/check-view-abi.py "$$artifact" || exit 1; \
	  limit=1200000; if [ "$$v" = chat-view ]; then limit=2500000; fi; \
	  bytes=$$(wc -c < "$$artifact"); \
	  echo "$$v: $$bytes bytes (limit $$limit)"; \
	  test "$$bytes" -le "$$limit" || exit 1; \
	done

## builds every VIEW_LINKABLE crate for wasm32-unknown-unknown, plus the
## exported view probe of view-guest, then fails if the normal wasm32 dependency
## tree of any of them names a VIEW_FORBIDDEN crate. Omit DWARF from these
## debug WASM artifacts so the exported probe fits the host module-size limit.
view-wasm-check:
	@for crate in $(VIEW_LINKABLE); do \
	  CARGO_PROFILE_DEV_DEBUG=0 $(CARGO) build --target wasm32-unknown-unknown -p $$crate || exit 1; \
	done; \
	CARGO_PROFILE_DEV_DEBUG=0 $(CARGO) build --target wasm32-unknown-unknown -p view-guest --example exported_view || exit 1; \
	reached=""; \
	for crate in $(VIEW_LINKABLE); do \
	  tree=$$($(CARGO) tree --target wasm32-unknown-unknown -e normal -p $$crate --prefix none) || exit 1; \
	  for dep in $(VIEW_FORBIDDEN); do \
	    if echo "$$tree" | grep -q "^$$dep v"; then reached="$$reached $$crate->$$dep"; fi; \
	  done; \
	done; \
	if [ -z "$$reached" ]; then \
	  echo "every view-linkable crate builds for wasm32 and stays off the signing/identity and JavaScript graphs"; \
	else \
	  echo "view-linkable crates reach a forbidden dependency:$$reached"; \
	  exit 1; \
	fi

# The forge harness and contract gate need only this app program.
.PHONY: forge-wasm
forge-wasm:
	$(CARGO) build -p forge --target wasm32-unknown-unknown --release
