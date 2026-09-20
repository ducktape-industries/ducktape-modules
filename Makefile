# ducktape-modules — the guest artifacts.
#
# Every module here carries its own wasm guest port and, where it ships a
# materialized view, an index guest. `guest-builder` builds one out of THIS
# repository at the checkout's HEAD — never out of the working tree — and
# writes the canonical `component.wasm` / `index.wasm` and `guest.lock` into
# the module directory. So: commit and push first, then build, then commit the
# artifacts.
#
# The builder lives in ducktape-sdk; point this at the one you built.
GUEST_BUILDER ?= ../ducktape-sdk/target/release/guest-builder

# The consensus components (`guest` feature, `src/guest.rs`).
BUILDER_MODULES := \
  crates/modules/apps/call \
  crates/modules/apps/chat \
  crates/modules/apps/pages \
  crates/modules/apps/agent \
  crates/modules/apps/runs \
  crates/modules/apps/tasks \
  crates/modules/apps/boards \
  crates/modules/apps/automations \
  crates/modules/apps/inbox \
  crates/modules/apps/collaboration \
  crates/examples/directory

# The modules that additionally ship an index guest (`index-guest` feature,
# `src/index_guest.rs`). That file IS the declaration — this list must name
# exactly the crates that carry one.
INDEX_MODULES := \
  crates/modules/apps/call \
  crates/modules/apps/chat \
  crates/modules/apps/inbox \
  crates/modules/apps/pages \
  crates/modules/apps/runs \
  crates/modules/apps/tasks

.PHONY: wasm-modules wasm-modules-check

## rebuild every committed guest artifact in place.
wasm-modules:
	@for m in $(BUILDER_MODULES); do $(GUEST_BUILDER) $$m || exit 1; done
	@for m in $(INDEX_MODULES); do $(GUEST_BUILDER) --index $$m || exit 1; done

## the drift gate between an artifact and its source: rebuild every guest out
## of the repository at HEAD, seeded from its committed guest.lock, into a temp
## directory and compare with the committed bytes. `--out` leaves the module
## directory (lock included) untouched, so the tree stays clean under the check.
## Needs the wasm32 target and a pushed HEAD.
#
# Every guest is rebuilt and compared before this reports, so ONE run names
# every stale artifact — a guest whose dependency moved is rarely alone.
wasm-modules-check:
	@out=$$(mktemp -d); trap 'rm -rf "$$out"' EXIT; \
	stale=""; checked=0; \
	for m in $(BUILDER_MODULES); do \
	  $(GUEST_BUILDER) $$m --out "$$out/$$(basename $$m).component.wasm" >/dev/null || exit 1; \
	  checked=$$((checked + 1)); \
	  cmp -s $$m/component.wasm "$$out/$$(basename $$m).component.wasm" || stale="$$stale $$m"; \
	done; \
	for m in $(INDEX_MODULES); do \
	  $(GUEST_BUILDER) --index $$m --out "$$out/$$(basename $$m).index.wasm" >/dev/null || exit 1; \
	  checked=$$((checked + 1)); \
	  cmp -s $$m/index.wasm "$$out/$$(basename $$m).index.wasm" || stale="$$stale --index $$m"; \
	done; \
	if [ -n "$$stale" ]; then \
	  echo "these committed guests do not match a rebuild of their source:"; \
	  set -- $$stale; \
	  while [ $$# -gt 0 ]; do \
	    if [ "$$1" = "--index" ]; then echo "    $(GUEST_BUILDER) --index $$2"; shift 2; \
	    else echo "    $(GUEST_BUILDER) $$1"; shift; fi; \
	  done; \
	  echo "  make wasm-modules refreshes the set; commit the artifacts with their locks."; \
	  exit 1; \
	fi; \
	echo "$$checked committed guest artifacts match a rebuild of their source at HEAD"
