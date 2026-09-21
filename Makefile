# modules — the wasm32 gates.
CARGO ?= cargo

# What a program links: abi and guest build for wasm32 with nothing else.
PROGRAM_LINKABLE := abi guest

# The programs, by manifest path: chat still pins the old sdk's `identity`, so
# the bare name is ambiguous until chat is a program.
PROGRAMS := crates/system/modules crates/system/valset crates/system/identity crates/app/forge

# The views: cdylibs for wasm32-unknown-unknown the desktop loads from a file.
VIEWS := chat-view

# What a wasm32 view may link. A crate a view links must never reach the
# signing/identity graph (blst does not build for wasm32, and a view has no
# business holding keys).
VIEW_LINKABLE := ducklink view-wire view-guest design
VIEW_FORBIDDEN := blst commonware-cryptography

.PHONY: program-wasm-check wasm-programs wasm-views view-wasm-check

## builds abi and guest for wasm32-unknown-unknown.
program-wasm-check:
	@for crate in $(PROGRAM_LINKABLE); do \
	  $(CARGO) build --target wasm32-unknown-unknown -p $$crate || exit 1; \
	done; \
	echo "abi and guest build for wasm32"

## builds every program for wasm32 under target/wasm32-unknown-unknown/release/.
wasm-programs:
	@for p in $(PROGRAMS); do $(CARGO) build --release --target wasm32-unknown-unknown --manifest-path $$p/Cargo.toml || exit 1; done

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
