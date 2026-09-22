# modules — the wasm32 gates. Every wasm artifact is a build output under
# $(BUILD_TARGET); none is committed.
CARGO ?= cargo
WASM_OPT ?= wasm-opt

# What a program links: abi and guest build for wasm32 with nothing else.
PROGRAM_LINKABLE := abi guest

# The ducktape checkout the probe fixture the founding suite seats is copied
# from (crates/kernel/fixtures, `make kernel-fixtures` there). The probe is a
# dev-dependency only, which cargo cannot build for wasm32 from here.
DUCKTAPE ?= ../core

# Every program, system and app: root members whose program ABI (the guest
# glue, the `alloc`/`call` exports, the `ducktape.*` imports) sits behind their
# `program` feature. Their views link the same crates with the feature off.
# One cargo invocation per program: forge links chat and identity links
# module-registry, and `-p a -p b --features program` in one call would unify
# `program` into the other's link (two `alloc`/`call`).
PROGRAMS := module-registry valset identity chat forge

# Views are wasm32 cdylibs. Chat and Forge ride their own programs;
# Settings rides the registry.
VIEWS := chat-view members-view node-view explorer-view settings-view forge-view

# What a wasm32 view may link. A crate a view links must never reach the
# signing/identity graph (blst does not build for wasm32, and a view has no
# business holding keys). The system crates are here because the system views
# read their contracts: module-registry's signing deps are dev-only, and `-e
# normal` below is what says so. Every program crate is linked with `program`
# off, which is what a plain `-p` build below checks.
VIEW_LINKABLE := ducklink view-wire view-guest design module-registry valset identity settings-view chat forge
VIEW_FORBIDDEN := blst commonware-cryptography wasm-bindgen js-sys web-sys

# Cargo uses this directory for both workspaces.
BUILD_TARGET := $(abspath $(or $(CARGO_TARGET_DIR),target))
RELEASE := $(BUILD_TARGET)/wasm32-unknown-unknown/release
# The artifacts wasm-modules leaves there (the target dir may hold others).
ARTIFACTS := $(foreach a,$(PROGRAMS) $(VIEWS),$(subst -,_,$(a)).wasm)

# A wasm artifact must be the same bytes from any checkout on any machine:
# panic locations would otherwise carry this checkout's, cargo's and the
# toolchain's absolute paths.
CARGO_HOME_DIR := $(or $(CARGO_HOME),$(HOME)/.cargo)
SYSROOT := $(shell rustc --print sysroot)
WASM_RUSTFLAGS := --remap-path-prefix=$(CURDIR)=/build --remap-path-prefix=$(CARGO_HOME_DIR)=/cargo --remap-path-prefix=$(SYSROOT)=/rustc
WASM_BUILD := RUSTFLAGS="$(WASM_RUSTFLAGS)" $(CARGO) build --target-dir $(BUILD_TARGET) --target wasm32-unknown-unknown --release

.PHONY: program-wasm-check wasm-programs probe-fixture wasm-views view-wasm-check test

## builds abi and guest for wasm32-unknown-unknown.
program-wasm-check:
	@for crate in $(PROGRAM_LINKABLE); do \
	  $(CARGO) build --target wasm32-unknown-unknown -p $$crate || exit 1; \
	done; \
	echo "abi and guest build for wasm32"

## builds every program (with `program` on) into $(RELEASE)/<name>.wasm. The
## founding suite reads the boot set from there.
wasm-programs:
	@for p in $(PROGRAMS); do \
	  $(WASM_BUILD) -p $$p --features program || exit 1; \
	done

## the whole suite: the founding suite runs the bytes wasm-programs built.
test: wasm-programs
	$(CARGO) test --workspace

.PHONY: wasm-modules wasm-reproducible

## builds every program and every view under $(RELEASE)/, unpacked. Packing
## a view into its program is genesis's job: qa's `make pack` runs view-pack
## over these outputs.
wasm-modules: wasm-programs wasm-views
	@cd $(RELEASE) && ls -l $(ARTIFACTS)

## builds every program and view twice, the second time from a fresh target
## directory, and requires the same sha256 for every artifact and no absolute
## path of this checkout or this home inside any of them.
wasm-reproducible:
	$(MAKE) wasm-modules
	@cd $(RELEASE) && sha256sum $(ARTIFACTS) > first.sha256 && cat first.sha256
	$(MAKE) wasm-modules CARGO_TARGET_DIR=$(BUILD_TARGET)/repro
	@cd $(BUILD_TARGET)/repro/wasm32-unknown-unknown/release && sha256sum $(ARTIFACTS) > second.sha256 && cat second.sha256
	@diff $(RELEASE)/first.sha256 $(BUILD_TARGET)/repro/wasm32-unknown-unknown/release/second.sha256 && echo "every program and view rebuilds to the same bytes"
	@for a in $(ARTIFACTS); do f=$(RELEASE)/$$a; \
	  if strings $$f | grep -qE "$(CURDIR)|$(HOME)"; then echo "$$f embeds an absolute path"; strings $$f | grep -E "$(CURDIR)|$(HOME)" | head -3; exit 1; fi; \
	done; echo "no program or view embeds a path of this checkout or home"

## refreshes the probe fixture the founding suite seats as the authority,
## from the ducktape checkout at $(DUCKTAPE).
probe-fixture:
	cp $(DUCKTAPE)/crates/kernel/fixtures/wasm/fixture_probe.wasm crates/system/module-registry/tests/

## builds every view for wasm32 under $(RELEASE)/.
wasm-views:
	@for v in $(VIEWS); do \
	  $(WASM_BUILD) -p $$v || exit 1; \
	  artifact="$(RELEASE)/$$(echo $$v | tr - _).wasm"; \
	  WASM_OPT="$(WASM_OPT)" tools/optimize-view.sh "$$artifact" || exit 1; \
	  python3 tools/check-view-abi.py "$$artifact" || exit 1; \
	  limit=1200000; case $$v in chat-view|forge-view) limit=2500000;; settings-view) limit=1300000;; esac; \
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
