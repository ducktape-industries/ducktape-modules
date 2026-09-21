# modules — the wasm32 gates.
CARGO ?= cargo

# What a program links: abi and guest build for wasm32 with nothing else.
PROGRAM_LINKABLE := abi guest

# The boot set: its own wasm32 workspace, its bytes committed beside it.
SYSTEM := crates/modules/system

# The ducktape checkout the probe fixture the `modules` suite seats is copied
# from (crates/kernel/fixtures, `make kernel-fixtures` there).
DUCKTAPE ?= ../ducktape

# The app programs: each its own wasm32 workspace, like the boot set.
PROGRAMS := crates/app/chat-program

# The views: cdylibs for wasm32-unknown-unknown the desktop loads from a file.
VIEWS := chat-view

# What a wasm32 view may link. A crate a view links must never reach the
# signing/identity graph (blst does not build for wasm32, and a view has no
# business holding keys).
VIEW_LINKABLE := ducklink view-wire view-guest design
VIEW_FORBIDDEN := blst commonware-cryptography

.PHONY: program-wasm-check wasm-programs probe-fixture wasm-views view-wasm-check

## builds abi and guest for wasm32-unknown-unknown.
program-wasm-check:
	@for crate in $(PROGRAM_LINKABLE); do \
	  $(CARGO) build --target wasm32-unknown-unknown -p $$crate || exit 1; \
	done; \
	echo "abi and guest build for wasm32"

## builds the boot set for wasm32 and refreshes its committed bytes, which a
## founding file names and the `modules` suite loads; then the app programs.
wasm-programs:
	$(CARGO) build --manifest-path $(SYSTEM)/Cargo.toml \
	  --target wasm32-unknown-unknown --release
	cp $(SYSTEM)/target/wasm32-unknown-unknown/release/*.wasm $(SYSTEM)/wasm/
	@for p in $(PROGRAMS); do \
	  $(CARGO) build --manifest-path $$p/Cargo.toml --target wasm32-unknown-unknown --release || exit 1; \
	done

## refreshes the probe fixture the `modules` suite seats as the authority,
## from the ducktape checkout at $(DUCKTAPE).
probe-fixture:
	cp $(DUCKTAPE)/crates/kernel/fixtures/wasm/fixture_probe.wasm crates/modules/tests/

## builds every view for wasm32 under target/wasm32-unknown-unknown/release/.
wasm-views:
	@for v in $(VIEWS); do $(CARGO) build --release --target wasm32-unknown-unknown -p $$v || exit 1; done

## builds every VIEW_LINKABLE crate for wasm32-unknown-unknown, plus the two
## exported probes of view-guest, then fails if the normal wasm32 dependency
## tree of any of them names a VIEW_FORBIDDEN crate.
view-wasm-check:
	@for crate in $(VIEW_LINKABLE); do \
	  $(CARGO) build --target wasm32-unknown-unknown -p $$crate || exit 1; \
	done; \
	$(CARGO) build --target wasm32-unknown-unknown -p view-guest --example exported || exit 1; \
	$(CARGO) build --target wasm32-unknown-unknown -p view-guest --example exported_view || exit 1; \
	reached=""; \
	for crate in $(VIEW_LINKABLE); do \
	  tree=$$($(CARGO) tree --target wasm32-unknown-unknown -e normal -p $$crate --prefix none) || exit 1; \
	  for dep in $(VIEW_FORBIDDEN); do \
	    if echo "$$tree" | grep -q "^$$dep v"; then reached="$$reached $$crate->$$dep"; fi; \
	  done; \
	done; \
	if [ -z "$$reached" ]; then \
	  echo "every view-linkable crate builds for wasm32 and stays off the signing/identity graph"; \
	else \
	  echo "view-linkable crates reach the signing/identity graph:$$reached"; \
	  exit 1; \
	fi
