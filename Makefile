# ducktape-sdk — the wasm32 gates.
CARGO ?= cargo

# What a program links: abi and guest must build for wasm32 with nothing
# else in the tree.
PROGRAM_LINKABLE := abi guest

# What a wasm32 view may link. A crate a view links must never reach the
# signing/identity graph (blst does not build for wasm32, and a view has no
# business holding keys); add a crate here when a view starts linking it.
VIEW_LINKABLE := duck-address refusal-class view-wire view-guest design duckfs-core
VIEW_FORBIDDEN := blst commonware-cryptography keyscheme

.PHONY: program-wasm-check view-wasm-check

## builds abi and guest for wasm32-unknown-unknown.
program-wasm-check:
	@for crate in $(PROGRAM_LINKABLE); do \
	  $(CARGO) build --target wasm32-unknown-unknown -p $$crate || exit 1; \
	done; \
	echo "abi and guest build for wasm32"

## builds every VIEW_LINKABLE crate for wasm32-unknown-unknown, plus the
## `export_app!` probe example of view-guest (the one place the exported view
## component is compiled in this tree), then fails if the normal wasm32
## dependency tree of any of them names a VIEW_FORBIDDEN crate (the build
## alone catches blst, not an identity crate that compiles).
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
